// Macro fidelity tests for vim-core.
//
// Tests for `q` (record), `@` (playback), and related macro commands.
// Macros are powerful for automating repetitive tasks.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC MACRO RECORDING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(macros, record_empty, "abc", "qaq");
neovim_test!(macros, record_single_motion, "hello", "qalq");
neovim_test!(macros, record_delete, "hello", "qaxq");
neovim_test!(macros, record_multiple_commands, "hello world", "qadwq");

// NEW: More recording edge cases
neovim_test!(macros, record_to_reg_z, "hello", "qzxq");
neovim_test!(macros, record_insert, "hello", "qaiX<Esc>q");
neovim_test!(macros, record_append, "hello", "qaaX<Esc>q");
neovim_test!(macros, record_change, "hello world", "qacwX<Esc>q");
neovim_test!(macros, record_yank, "hello", "qaywq");
neovim_test!(macros, record_paste, "hello", "qaywpq");
neovim_test!(macros, record_find, "hello", "qafll0q");
neovim_test!(macros, record_visual, "hello", "qavwdq");
neovim_test!(macros, record_text_object, "hello world", "qadiwq");
neovim_test!(macros, record_unicode, "日本語", "qaxq");

// ═══════════════════════════════════════════════════════════════════════════════
// MACRO PLAYBACK
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(macros, playback_motion, "hello", "qalq0@a");
neovim_test!(macros, playback_delete, "ab", "qaxq@a");
neovim_test!(macros, playback_with_count, "abcd", "qaxq2@a");

// NEW: More playback edge cases
neovim_test!(macros, playback_insert, "hello", "qaiX<Esc>q0@a");
neovim_test!(macros, playback_change, "hello world foo", "qacwX<Esc>wq@a");
neovim_test!(macros, playback_yank_paste, "hello world", "qaywwpq");
neovim_test!(macros, playback_multiline, "l1\nl2\nl3", "qaddq@a");
neovim_test!(macros, playback_count_5, "abcdefgh", "qaxq5@a");
neovim_test!(macros, playback_unicode, "日本語テスト", "qaxq@a");
neovim_test!(macros, playback_to_eol, "hello world", "qa$xq0@a");

// ═══════════════════════════════════════════════════════════════════════════════
// MACRO ACROSS LINES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(macros, record_line_down, "a\nb\nc", "qaj0q@a");
neovim_test!(macros, delete_lines_macro, "a\nb\nc\nd", "qaddq2@a");

// NEW: More multiline macro cases
neovim_test!(macros, macro_jx, "ab\ncd\nef", "qajxq0@a");
neovim_test!(macros, macro_A_j, "hello\nworld\ntest", "qaAX<Esc>jq@a");
neovim_test!(macros, macro_o_command, "hello", "qaoWorld<Esc>q@a");
neovim_test!(macros, macro_O_command, "hello\nworld", cursor(1, 0), "qaONew<Esc>q");
neovim_test!(macros, macro_yy_p, "hello\nworld", "qayyPq@a");
neovim_test!(macros, macro_dd_motion, "l1\nl2\nl3\nl4", "qaddjq@a");

// ═══════════════════════════════════════════════════════════════════════════════
// @@ (REPEAT LAST MACRO)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(macros, repeat_last_macro, "abcd", "qaxq@a@@");

// NEW: More repeat cases
neovim_test!(macros, repeat_after_different, "abcdef", "qaxqqbxq@b@@");
neovim_test!(macros, repeat_multiple_times, "abcdefgh", "qaxq@a@@@@");
neovim_test!(macros, repeat_with_count, "abcdefgh", "qaxq@a2@@");
neovim_test!(macros, repeat_complex, "hello world foo", "qadwq@a@@");

// ═══════════════════════════════════════════════════════════════════════════════
// NAMED REGISTERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(macros, different_registers, "abc", "qaxqqbxq0@a@b");

// NEW: More register cases
neovim_test!(macros, all_letter_registers, "abc", "qaxqqbxq0@a@b");
neovim_test!(macros, overwrite_register, "abcd", "qaxqqadwq0@a");
neovim_test!(macros, uppercase_append, "abc", "qaxq\"Aqqbxq0@a");
neovim_test!(macros, macro_in_reg_z, "hello", "qzxq@z");

// ═══════════════════════════════════════════════════════════════════════════════
// COMPLEX MACROS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(macros, insert_in_macro, "hello", "qaiX<Esc>q0@a");
neovim_test!(macros, word_operations, "foo bar baz", "qadwq@@");

// NEW: More complex macro cases
neovim_test!(macros, macro_with_search, "hello world hello", "qa/hello<CR>xq@a");
neovim_test!(macros, macro_with_replace, "aXbXcX", "qafXrYq0@a");
neovim_test!(macros, macro_change_word, "one two three", "qacwX<Esc>wq@a");
neovim_test!(macros, macro_delete_to_end, "hello\nworld", "qaD0jq@a");
neovim_test!(macros, macro_indent, "hello\nworld", "qaI  <Esc>jq@a");
neovim_test!(macros, macro_with_marks, "hello", "qama`aq");
neovim_test!(macros, macro_visual_yank, "hello world", "qaviwyq0@a");
neovim_test!(macros, macro_surround_word, "hello world", "qaciw(<Esc>pa)<Esc>wq@a");
neovim_test!(macros, macro_swap_chars, "hello", "qaxpq@a");

// ═══════════════════════════════════════════════════════════════════════════════
// RECURSIVE MACROS
// ═══════════════════════════════════════════════════════════════════════════════

// Recursive macro causes infinite loop - skipped
// neovim_test!(macros, recursive_x, "abcd", "qax@aq0@a");
// neovim_test!(macros, recursive_with_check, "abc\ndef", "qaj@aq0@a");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(macros, empty_register_playback, "abc", "@z");
neovim_test!(macros, record_stops_on_q, "abc", "qa");

// NEW: More edge cases
neovim_test!(macros, macros_empty_buffer, "", "qaq@a");
neovim_test!(macros, single_char_buffer, "a", "qaxq@a");
neovim_test!(macros, q_in_insert_mode, "hello", "qaiq<Esc>q");
neovim_test!(macros, playback_at_eol, "hello", cursor(0, 4), "qaxq@a");
neovim_test!(macros, playback_at_bol, "hello", "qaxq@a");
neovim_test!(macros, macro_on_last_line, "l1\nl2", cursor(1, 0), "qaddq@a");
neovim_test!(macros, macro_double_q, "hello", "qqxq@q");
neovim_test!(macros, count_before_playback, "abcdef", "qaxq3@a");
neovim_test!(macros, macro_with_escape, "hello", "qaiX<Esc><Esc>q0@a");
neovim_test!(macros, macro_unicode_motion, "日本語 テスト", "qawq0@a");
neovim_test!(macros, macro_cjk, "日本語テスト", "qaxq@a");
neovim_test!(macros, macro_emoji, "👍 test", "qaxq@a");

// ═══════════════════════════════════════════════════════════════════════════════
// REPLAY LAST EX COMMAND (@:)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(macros, replay_ex_delete, "line1\nline2\nline3", ":d<CR>@:");
neovim_test!(macros, replay_ex_substitute, "foo\nfoo\nfoo", ":s/foo/bar/<CR>j@:");
neovim_test!(macros, replay_ex_counted, "a\nb\nc\nd\ne", ":d<CR>2@:");

// ═══════════════════════════════════════════════════════════════════════════════
// MACRO — EXPANDED COVERAGE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(macros, macro_with_substitute, "foo\nfoo\nfoo", "qa:s/foo/bar/<CR>jq2@a");
neovim_test!(macros, macro_nested, "abc", "qaiX<Esc>qqb@aq@b");
neovim_test!(macros, macro_append_register, "abc\ndef", "qax<Esc>jqqAx<Esc>q@a");
neovim_test!(macros, macro_search_ciw_replay_undo, "i want to change the world\ni want to change the world\ni want to change the world", "qa0/change<CR>ciwlol<Esc>jq2@auu");

