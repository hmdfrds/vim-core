# Changelog

## v0.10.0

### Breaking Changes

- Owned text in the API is now vim-core's own `VimString` instead of
  compact_str's `CompactString`, and compact_str (now 0.10) is a private
  dependency that vim-core no longer exposes. Every field, variant,
  parameter and return that held a `CompactString` holds a `VimString`,
  among them `Effect`, `HostRequest`, `HostResult`, `HostEvent`,
  `ExCommand`, `VimError`, `IndentResult` and `VimValue::String`, and each
  `impl Into<CompactString>` parameter is `impl Into<VimString>`. Code that
  reads the text as a `&str` (`&*x`, `x.as_str()`, `x == "foo"`, `format!`)
  or builds it with `.into()` from a std string compiles unchanged. A host
  that keeps its own `CompactString` converts through `&str`:
  `VimString::from(cs.as_str())`. `CompactString::const_new` becomes
  `VimString::from_static`, `format_compact!` becomes `format_vim!`, and a
  `StateBackend` implementation changes the five methods that named
  `CompactString`.
- `vim_core::regex::VimRegexErrorKind`: the `detail` of `InvalidQuantifier`,
  `InvalidCollectionRange`, `InvalidCollectionCharCode`, `InvalidCharCode`,
  `PatternTooComplex` and `InternalError`, and `UnknownPosixClass::name`,
  are `Box<str>`. Read them as `&str` as before; build them with `.into()`.
- `ExCommand::LetMapleader { leader }` is now
  `ExCommand::LetLeader { local, value }`, which also covers
  `:let maplocalleader` and holds the expression as written. Match
  `LetLeader` and read the leader back with `VimEngine::leader_keys()` or
  `local_leader_keys()`.
- `ExCommand::Global` holds the rest of the line as `commands: VimString`
  instead of one parsed `command: Box<ExCommand>`. To see the commands,
  split `commands` with `grammar::split_ex_pipeline` and parse each piece.
- `ExCommand::Map`, `Unmap`, `MapClear` and `AbClear` gain a `buffer` field
  and `ExCommand::Abbreviate` a `unique` field. Match them with `..`.
- `grammar::split_ex_pipeline` returns `Vec<Cow<'_, str>>` (was
  `Vec<&str>`), since a piece can lose the backslashes, Ctrl-Vs and comment
  Vim removes. Read each piece with `&*piece` or `.as_ref()`. It now splits
  as Vim's command table says, and `parse_ex_command` and
  `parse_ex_command_with_modifiers` take one piece of it: they no longer
  strip a trailing `"` comment, so split a line before parsing it.
- `VimError::GlobalRecursionLimitExceeded` (E5101) is removed. Nested `:g`
  gives `VimError::CommandTooRecursive` (E169), as every other nested
  command does.
- `SourceContext` is `#[non_exhaustive]`. Build it with
  `SourceContext::new(file, line)` and match it with `..`.
- `AbbrevEntry` gains a public `trust` field. A struct literal adds
  `trust: TrustTag::default()`.
- `ModifierFlags` is `u32` (was `u16`), so `bits()`, `from_bits()`,
  `from_bits_retain()` and `from_bits_truncate()` take and give `u32`.
  Serialized values keep their numbers.
- `OperatorContext`, `RangeResult` and `ViewportInfo` are
  `#[non_exhaustive]`. Build a `ViewportInfo` with
  `ViewportInfo::new(first_line, height, width)` and an `OperatorContext`
  with `OperatorContext::new`. `Command::OperatorTextObject` gains a
  `force_type` field, `None` for now.
- `MapModePrefix` gains `InsertCommand`, for `:map!` and its family, so
  `MapModePrefix::ALL` has 9 entries.
- `vim_core::commands::ex::global` is removed: the engine runs `:g` itself.
- Some behaviour a host may rely on changed too: 'langremap' is on by
  default, a Ctrl-V in a mapping quotes the next key, and in a config a Vim
  command vim-core lacks gives E319 instead of going to the host. See
  Behaviour Changes.

### Deprecated

- `VimEngine::source_config_text` is deprecated and goes in v0.11.0. Use
  `VimEngine::source_script(ConfigText::new("", text), &mut TrustAll::new()).response`,
  or pass a `ConfigHost` of your own to judge each command. `source_script`
  does not add the closing `Config file sourced` message.

### New

- Config loading. `VimEngine::source_script` and
  `HostSession::source_script` run a vimrc the way Vim sources a file, given
  its name, text and a `TrustTag` of the host's as a `ConfigText`. The lines
  are read as Vim reads them: a byte order mark, Vim's CR LF rule, `\`
  continuation lines and `"\ ` comments, and the lines of a heredoc,
  `:function`, `:append` or `{` block are taken as text and never run.
  `:if`, `:elseif`, `:else`, `:endif` and `:finish` work, with Vim's error
  flow: an error ends its line and, inside a block, skips to the end of the
  outermost block, and `:silent!` hides it and skips nothing. Conditions and
  `:let` evaluate Vim script expressions (`!`, `&&`, `||`, comparisons,
  strings with escapes such as `"\<Space>"`, `has()`, `exists()`, `&option`,
  `v:version`). `:let` and `:unlet` set and remove `g:`, `b:` and plain
  variables and set options, with `=`, `+=`, `-=`, `.=` and `..=`. A nested
  `:source` runs in place, up to 200 files deep. Every error names its place
  in Vim's form, `a.vim[12]..b.vim` with its line, in `ShowError.source`,
  and goes to the message history under Vim's
  `Error detected while processing` header. The returned `ConfigOutcome`
  counts the errors and the commands judged, refused and claimed.
- The policy hook. `ConfigHost::judge` sees every command a config would
  run, once, after the engine's own checks and before it changes anything:
  commands after a `|`, on continuation lines, inside blocks and in files
  the config sources. It answers `Verdict::Allow` (the default),
  `Verdict::Refuse(reason)`, an error at the command's line that ends its
  line as any error does, or `Verdict::Claim`, which skips the command for a
  host that runs it itself. The `ConfigCommand` it gets names the file, tag,
  depth, line, text and modifiers, a typed `ConfigAction` (the options set
  and whether Vim keeps each out of reach of untrusted text, a mapping, an
  abbreviation, the leader, a `:source`, a host command and more) and
  `Reach` flags such as options, mappings, the shell, files and the host. A
  command that runs others, such as `:g` or `:windo`, reaches what each of
  them does; `ExCommand::reach()` gives the same for any parsed command.
  `ConfigHost::source` decides what a `:source` in the config does
  (`SourceReply::Text`, `Done`, `Refuse` or the default `Defer`, a
  `ReadConfigFile` request) and `ConfigHost::has_feature` answers `has()`
  for the host's own features. `TrustAll` allows everything. Mappings and
  abbreviations keep the tag of the config that defined them, read with
  `MappingEntry::trust()` and `AbbrevEntry::trust`.
- `VimEngine::mapping_reach(modes, rhs, kind, flags)`, and
  `MapDefinition::reach` for each mapping a config defines, say what the
  keys of a mapping can run when pressed: each command line or `<Cmd>`
  command with its `Reach`, whether the keys leave a line open for the user
  to finish, the `<Action>(..)` and `<Plug>(..)` names they hold, and, as
  `Opaque`, what cannot be followed, such as `<expr>`, a recursive mapping,
  `@`, the `!` operator or `:normal`. Each mode is followed as the engine
  reads it, with the leader and 'langmap' in force, and commands inside
  `:g`, `:windo` and the like as deep as the engine runs them. The analysis
  is bounded in keys, branches and work.
- The run-time judge. `VimEngine::set_command_line_judge` (also on
  `VimSession`, so on `HostSession`) installs a `CommandLineJudge`, asked
  about each command of a command line that keys from a mapping with a
  non-default `TrustTag` opened, edited or ran, before any of the line runs,
  then about each command of a `:g` or `:v` list, the command of `:windo`,
  `:bufdo`, `:tabdo`, `:sx` and `:sy`, and the lines of `:@r`. A refusal is
  an error and nothing of the line from that command on runs. A line only
  typed, or made by mappings with the default tag, never reaches the judge,
  and with no judge installed every line runs as before. Keys from a mapping
  carry its tag, and a line keeps the tags of every key that opened, edited
  or ran it, through its history entry, `@:`, `:@:`, a recalled line, the
  command-line window, and the keys `:normal` and a register run from it. A
  mapping or abbreviation that a tagged line defines keeps the strictest of
  the line's tags, which vim-core takes to be the highest number, so a host
  numbers its tags from the most trusted up. Text a host gives for a
  `:source` as `SourceReply::Text` runs with the tag in that reply, not the
  tag of the line that ran the `:source`; text sent back for a
  `ReadConfigFile` request runs with the default tag.
- `VimEngine::reset_config_state` and `HostSession::reset_config_state`
  clear what a config defines before it is sourced again: the user's
  mappings, every abbreviation, the handler map and the leaders. Mappings a
  host registered, options, other variables, registers and marks stay.
- `VimString`, at the crate root, in `primitives` and in the prelude, with
  the `format_vim!` macro. It has the size and inline storage of the type it
  replaces, derefs to `str`, converts from and into the std string types and
  has the same serde form, so a `VimState` saved by v0.9.0 still loads.
- Also added: `ViewportInfo::new`, `SourceContext::new`,
  `VimEngine::abbreviations()`, `VimEngine::leader_keys()` and
  `local_leader_keys()`, `MappingEntry::with_trust`, `AbbrevTable::defines`,
  `ExCommand::needs_buffer`, the `ExCommand` variants `Block`, `Finish` and
  `Mark`, the `SetAssignment` variants `InvertBool`, `Reset` and `Invalid`,
  `CmdFlags::CTRLV`, `XFILE`, `EXPR_ARG` and `SBOXOK`,
  `ModifierFlags::VERBOSE`, `KEEPMARKS` and `UNSILENT`, and `VimError`
  variants for the Vim errors the new code gives, among them E30, E169,
  E319, E519, E24 and E31.

### Behaviour Changes

- A command line splits at `|` the way Vim's command table says. A command
  that Vim ends at `|` ends there, so `:filetype on | set ic`,
  `:pwd | set ic` and `:syntax on | set ic` run the `:set`, and
  `:let x = 1 | echo x` sends only `let x = 1` to the host. An expression
  ends where it ends, so `:echo "a|b"` echoes `a|b`, where it gave E471. A
  `|` inside a pattern stays in it: `:/a|b/d | set ic`, `:g/x/s/a|b/c/ | w`,
  `:vimgrep /a|b/ % | cw`, `:match Foo /a\|b/ | set ic` and `:sort /a|b/`.
  `:wincmd`, `:help`, `:delfunction` and `:function` run the command after
  their `|`, and a command after a modifier the engine does not run ends
  where that command ends. A command that takes the rest of the line keeps
  it: `:g/pat/normal x | w` gives `:normal` the keys `x | w`.
- A `"` after a command that allows a comment starts one, which needs no
  blank before it and runs to the end of the line, `|` included:
  `:set tw=20" c | set ic` sets only 'textwidth', `:s/a/b/g " note | w` is
  one command, `:run "foo bar"` sends `run` to the host, and `:sp "x"` is
  `:split` with a comment. A typed comment line such as `:" note` runs
  nothing, where it went to the host.
- In a command that ends at `|`, `\"` gives `"`, `\\|` gives `\|` and a
  Ctrl-V is dropped except in mappings and file names, so `:set ic^V|x` is
  `:set ic|x`. A blank after a backslash stays at the end of an argument
  (`:e foo\ `), `:sp \|x` splits to the file `|x`, and `:set ic |` and
  `:w |` no longer read `|` as an argument.
- An error ends a typed command line, an `@:` and a line of `:@r`:
  `:set nosuch | set ic` leaves 'ignorecase' off. A `<Cmd>` mapping goes on
  after an error. A `|` line is one undo step.
- `:g` and `:v` with no line to run on give the message
  `Pattern not found: x`, or for `:v` `Pattern found in every line: x`,
  where they gave E486. A `:s` that finds nothing inside `:g` is no error,
  an error ends `:g` once its line is done, a `:g` inside `:g` with a range
  gives E147, and the report counts the substitutions or the lines added or
  removed.
- Text after the flags and count of `:s`, `:&` or `:~` gives
  `E488: Trailing characters`, so `:s/a/b/ g` is an error and
  `:s/o/0/x | set ic` runs nothing.
- A typed `:if`, `:elseif`, `:else`, `:endif`, `:while`, `:endwhile`,
  `:for`, `:endfor`, `:try`, `:catch`, `:finally` or `:endtry` gives E319
  and a typed `:finish` E168, where they went to the host. An empty command
  line, or one of blanks or modifiers only, does nothing, where it moved the
  cursor to the first non-blank. A no-break space before a command no longer
  starts it, and `:let x =<< END | set ic` goes to the host whole.
- `:let mapleader = {expr}` and `:let maplocalleader = {expr}`, with `g:` or
  without, take an expression: `"\<Space>"` is Space where it gave E521, a
  value of several keys is a leader of several keys, an unquoted `,` or `\`
  gives E15, and `:let maplocalleader` no longer goes to the host.
- `:set` stops at its first bad argument and gives Vim's message with the
  argument as written: `:set nosuch ts=7` gives
  `E518: Unknown option: nosuch` and leaves 'tabstop', where it gave E492
  and set 'tabstop'. Its arguments follow Vim: `:set tw` shows the value and
  `:set tw!` gives E488; `:set ic=1`, `:set notw` and `:set invtw` give E474
  and `:set tw=-1` E487; `=` reads `0x8`, `010` and `0b1000` as Vim numbers;
  blanks before an operator are allowed (`:set sw =2`), an escaped blank
  stays in the value, and a string value loses the backslash before each
  character; `:set fo&`, `:set tw<` and `:set all&` give E319; 'belloff'
  takes `all`. Each `:set`, `:setlocal` and `:setglobal` of a `|` line
  writes the layers its own name says.
- `:set` of an option Vim has and vim-core lacks, such as `:set mouse=a`,
  `:set nocompatible` or `:set t_Co=256`, does nothing, where it gave E518,
  and asking for its value gives `E519: Option not supported`.
  `:set cms=#%s` sets 'commentstring', and `:set commentstring?` shows it.
- Map, unmap and abbreviation commands read `<buffer>`, `<nowait>`,
  `<silent>`, `<special>`, `<script>`, `<expr>` and `<unique>` in any order
  and only in this exact case, so `<Silent>` is part of the keys. `<buffer>`
  maps in the current buffer, `<unique>` gives E227, E225 or E226,
  `:mapclear <buffer>` clears only the buffer's mappings, where it cleared
  them all, and `:mapclear x` gives E474. `<buffer>` and `<expr>` on an
  abbreviation command give E319, where they were part of the trigger.
- The right-hand side of a map or abbreviation command keeps its trailing
  blanks, so `:nnoremap x l ` moves two columns and `:nnoremap x y | set ic`
  maps x to `y `. `:unmap` takes its whole argument, so `:unmap x ` names
  the keys `x `.
- A Ctrl-V in the keys or right-hand side of a mapping makes the next
  character a key, as in Vim: `:nnoremap x a^V|b<Esc>` appends `|b`, and
  `^V<CR>` is the four keys `<CR>`, where a Ctrl-V was a key of its own. The
  same holds in `register_host_mappings` and in the keys a host returns for
  an `<expr>` mapping.
- `<Nop>`, in any case, as a whole right-hand side maps to nothing, and a
  mapping to nothing takes its keys and does nothing, where it pressed
  Escape and so left Insert mode, Visual mode or the command line.
- `:unmap` gives `E31: No such mapping` and `:unabbreviate`
  `E24: No such abbreviation` when they remove nothing, where they were
  silent. When no keys match, they remove the entries whose right-hand side
  is the argument, so `:unmap y` after `:map x y` removes x.
- `:map!`, `:noremap!`, `:unmap!` and `:mapclear!` work on Insert and
  Command-line mode, where they worked on Normal, Visual and
  Operator-pending mode. `:vmap`, `:xmap` and `:smap` share Visual and
  Select mode as in Vim: after `:map x dd`, `:xunmap x` leaves x mapped in
  Select mode, and `:vunmap x` removes an `:xmap x`.
- `:abclear` clears every abbreviation, where it cleared only the
  `:abbreviate` ones, and `:iabclear`, `:cabclear`, `:iunabbrev` and
  `:cunabbrev` take their own mode off each. `:iabbrev a b` and
  `:cabbrev a c` keep both, and `:abbreviate` lists them all.
  `:abclear <buffer>` clears nothing and `:abclear x` gives E474.
- `:norm$`, with no blank after the name, runs `$`, where it gave E471.
- 'langremap' is on by default, as in Vim, and is honoured: with 'langmap'
  set, every key a mapping gives is translated, its first key included,
  where the first was not. With 'langremap' off only typed keys are
  translated, not the keys of a mapping, a register or `feed_keys()`.
  `:set langnoremap` turns 'langremap' off, and `:set lnr?` shows
  'langnoremap', its inverse. A host that relied on the old default sets
  'nolangremap'.
- `@:` and `:@:` with no previous command line give
  `E30: No previous command line`, where they gave E34. A line becomes the
  last command line once it ran, and the line `@:` runs in is not stored.
- A `:source {file}` that the host answers with the file's text puts each of
  its messages and errors in the message history, the errors under Vim's
  `Error detected while processing {file}:` and `line    3:` entries, and
  gives each error a `SourceContext`. A file that sources itself stops with
  E169 after 199 files.
- `source_config_text` and `HostSession::source_config` run a config as
  `source_script` does: every command of a line runs, host requests come
  back as pending requests, and every message comes back. In a config, a Vim
  command vim-core lacks, such as `:syntax`, `:colorscheme` or `:autocmd`,
  gives `E319: Sorry, the command is not available in this version`, where
  it went to the host as a custom command; a command that is not Vim's, such
  as a host command, still goes to the host. `:verbose`, `:noautocmd`,
  `:keepmarks`, `:leftabove`, `:rightbelow` and `:unsilent` in a config run
  the command after them as without them.

### Bug Fixes

- Fixed an Insert or Replace that changed nothing, as `i<Esc>`, leaving an
  empty undo step, so the next `u` seemed to do nothing. It now leaves no
  step, as in Vim, and its `EndUndoGroup` carries no node. A `.` of it adds
  no step either, and `.` after `cw<Esc>` is one step. `o`, `O`, and `c`,
  `s` or `S` on an empty region still make a step, as in Vim.
- Fixed Insert and Replace with several cursors panicking or leaving the
  cursors in the wrong places. `a`, `A`, `I` and `gI` enter at each cursor's
  own place, `o`, `O`, `s` and `S` run at each cursor, Replace overwrites
  under each, line breaks keep the cursors in order, a counted or empty
  insert and `.` finish at each cursor, `<Esc>` steps each back on its own,
  and the primary cursor stays where the host cursor is. A counted Replace
  repeats over the characters it replaces, as in Vim.
- Fixed the commands of a `|` line running on the text as it was before the
  line: a later command used stale positions, so `:2 | d` deleted line 1,
  and some lines could crash the engine. Each command now runs on the text,
  cursor and marks the one before left.
- Fixed `:g` and `:v` running their command on the text as it was before the
  `:g`, which could corrupt the text or crash the engine. They now take the
  rest of the line, every `|` command in it, and run it on each marked line
  that is left, one line at a time, so `:g/^/m0` reverses the lines and
  `:g/x/s/a/b/|d` changes and deletes each line.
- Fixed commands nested without a bound, typed, from a register or from a
  mapping, crashing the engine on a small stack or never stopping. A command
  that `:g`, `:v`, `:sx`, `:sy`, `:windo`, `:bufdo` or `:tabdo` runs, a `:g`
  or `:@r` command list, `:normal`, `@:` and `:@:` each count a level, and
  past 30 levels the command gives `E169: Command too recursive` and does
  not run. The old limit of 10 on `:g` inside `:g`, and its E5101, are gone.
- Fixed a command line that runs `@:` running itself without end and
  crashing the engine. `@:` now runs the line before it, or gives E30.
- Fixed `:m` and `:t` that reach a last line with no newline after it
  joining two lines or losing a line break: `:2,3m0` on `a\nb\nc` gives
  `b\nc\na`. `:m` to just after its own lines, as `:2m2`, does nothing,
  where it gave E134.
- Fixed the cursor landing inside a Tab or a multi-byte character after a
  linewise delete such as `dd` with 'nostartofline'. It goes to the
  character at its screen column, or the last one of a shorter line.
- Fixed `)`, `e` and `E` that run off the end of a text ending in a newline
  leaving the cursor on that newline, from where a later motion could crash
  the engine. The cursor stops on the empty line after it, as in Vim.
- Fixed `<Esc>` typed while Insert mode waited for the argument of `<C-r>`,
  `<C-r>=`, `<C-k>` or `<C-g>` leaving the engine waiting, so the next
  Normal-mode key could crash it. The `<Esc>` is used up and Insert mode
  stays, as in Vim.
- Fixed Visual `*` and `#` with 'visualstar' not finding a selection that
  holds a backslash or a slash. The selection is now searched literally,
  line breaks included.
- Fixed `:mark {m}` and `:k{m}` setting no mark: `:mark` listed the marks
  and `:k` was unknown. They set the mark at the range's last line, with
  Vim's E471, E488 and E191.
- Fixed a command with trailing blanks, such as `t. ` or `@a `, being read
  as an unknown command and sent to the host.
- Fixed `:setglobal` in a config also writing the local value, which lost a
  local value a host had set.
- Fixed `:normal` in a `<Cmd>` mapping doing nothing.

### Known Limits

Where vim-core still differs from Vim 9.1:

- Command names are read in lowercase, so `:Set ic` is `:set` and `:N` is
  `:next`. A few short forms name other commands than in Vim: `:sm` is
  `:smap`, `:sn` `:snoremap`, `:su` `:sunmap`, `:sy on` is the engine's
  structural `:sy` rather than `:syntax on`, and `:kee` is the engine's
  multi-cursor `:keep`.
- `:s` takes `|` and `"` as its delimiter and refuses `_`, where Vim does
  the opposite. A delimiter inside `[...]` ends the pattern of `:s`, `:g`
  and similar commands. In rare cases `:syntax`, `:sort` with a bad
  argument, `:delfunction` and `:autocmd` end at a different `|` than in
  Vim, and an empty command between two bars, as in `:w||q`, is skipped
  where Vim prints the line.
- Typed `:if`, `:while`, `:for` and `:try` give E319, and a typed `:break`,
  `:continue`, `:endfunction`, `:return` or `:throw` goes to the host.
- In a `|` line or a `:g`, the commands after `:undo`, `:redo`, `:earlier`
  or `:later` do not run; a command the host runs, such as `:r` or a `:!`
  filter, runs after the rest of its line, which does not see its change;
  and `:normal` inside `:g` runs after the `:g`, on the lines it named, so
  `:g/a/normal dd` on `a`, `a`, `b` leaves `a` where Vim leaves `b`.
- Sourced files nest 200 deep and Ex commands 30 deep, each on its own, with
  E169 past either; Vim shares one budget of 200 and gives E192 for
  `:normal` past 'maxmapdepth'.
- In a config, `:while`, `:for` and `:try` give E319 and their lines never
  run, and every Vim command vim-core lacks, `:syntax`, `:colorscheme`,
  `:highlight`, `:autocmd`, `:command`, `:function`, `:call` and `:execute`
  among them, gives E319 and ends its line and, inside a block, the rest of
  the outermost block. So `if has('eval')`, `syntax on`, `set sw=2`, `endif`
  leaves 'shiftwidth' as it was. `:vim9script` gives E319 and ends the file,
  `:scriptencoding` does nothing, and a command that is not Vim's goes to
  the host where Vim gives E492.
- Expressions support what a vimrc's conditions and `:let` lines need.
  Lists, Dictionaries, lambdas, registers, `->` and the like give E319; only
  `has()` and `exists()` can be called; `has()` knows a few features;
  `$NAME` is always empty; of the `v:` variables only `v:version`,
  `v:versionlong`, `v:true` and `v:false` exist; `"\<F5>"` keeps its `<...>`
  text; groups and calls nest at most 300 deep.
- In a config, `:let` and `:unlet` handle only `g:`, `b:` and plain
  variables and options, and `:setlocal` acts like `:set`, as there is no
  buffer or window. Typed, `:unlet` and a `:let` of anything but the leaders
  still go to the host, and `:echo` shows its argument as written. A scope
  holds at most 10 000 variables.
- `:set` shows a value without Vim's indent, refuses a number out of range
  where Vim stores a fallback with the error, gives E319 for the `&`,
  `&vim`, `&vi` and `<` forms, and `:setglobal wrap!` inverts the value in
  use rather than the global one.
- An option Vim has and vim-core lacks, such as 'mouse', acts as one the
  build lacks: setting it does nothing and asking for it gives E519, where
  Vim shows its value. A terminal key code such as `t_ku` is no option and
  gives E518.
- `<buffer>` in a config gives E523, as there is no buffer. Abbreviations
  have no `<buffer>` or `<expr>` form, a `<script>` mapping does not remap
  at all, a map command without a right-hand side lists nothing, an
  abbreviation's replacement is inserted as text (`<CR>` in it is four
  characters), command-line abbreviations never expand, and a listing of
  abbreviations is laid out differently.
- In a config, `:verbose`, `:noautocmd`, `:keepmarks`, `:leftabove`,
  `:rightbelow` and `:unsilent` change nothing, and a command after
  `:filter`, `:sandbox`, `:noswapfile`, `:hide`, `:legacy` or `:vim9cmd`
  gives E319.
- A `:source` the host defers, which every typed `:source` is and which a
  `ConfigHost` gives by default, runs its file when the host answers with
  the text: the commands after the `:source` run first and its errors do not
  skip them. A bare `:source` asks the host for a file, where Vim 9.1
  sources the current buffer. A sourced file is named by its path as typed,
  not its full path.
- `source_config_text`, `HostSession::source_config` and a `:source` the
  host answers with the text end with the message `Config file sourced`,
  which Vim does not give, and a config without a name gets line labels
  before its errors.
- A register recorded with `q` holds the keys as 'langmap' translated them,
  so with 'langremap' on `@q` translates them again, and a typed key that
  waited as the start of a mapping and matched none goes through 'langmap' a
  second time.

### Tests

- Four multi-cursor Insert cases that were pinned as known failures now
  pass: `mc_A_appends_at_each_line_end`, `mc_A_different_line_lengths`,
  `mc_dot_repeats_append_semicolon_at_new_positions` and
  `mc_dot_repeats_substitute`. 78 known failures remain, all as recorded.
- New suites cover the config loader against Vim's sources, `|` lines and
  `:g` with a property test that runs a line whole and command by command,
  nesting at the 30-level limit on a 1 MiB stack, every mapping of a corpus
  of 1,608 configs pressed in each mode and checked against `mapping_reach`,
  and random tagged mappings pressed under judges that refuse or allow
  everything.

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
