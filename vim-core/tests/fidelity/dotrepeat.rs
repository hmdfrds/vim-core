// Dot repeat fidelity tests for vim-core.
//
// Tests for the `.` command which repeats the last change.
// The dot command is a core Vim feature for efficiency.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC DOT REPEAT
// ═══════════════════════════════════════════════════════════════════════════════

// Delete operations
neovim_test!(dotrepeat, dot_delete_char, "abcd", "x.");
neovim_test!(dotrepeat, dot_delete_word, "hello world foo", "dw.");
neovim_test!(dotrepeat, dot_delete_line, "line1\nline2\nline3", "dd.");
neovim_test!(dotrepeat, dot_delete_to_eol, "hello\nworld", "D.");

// Change operations
neovim_test!(dotrepeat, dot_change_word, "hello world foo", "cwX<Esc>w.");
neovim_test!(dotrepeat, dot_change_char, "abcd", "sX<Esc>l.");
neovim_test!(dotrepeat, dot_change_line, "line1\nline2\nline3", "ccX<Esc>j.");

// Insert operations
neovim_test!(dotrepeat, dot_insert, "hello", "iX<Esc>l.");
neovim_test!(dotrepeat, dot_append, "hello", "aX<Esc>l.");
neovim_test!(dotrepeat, dot_insert_line_start, "  hello\n  world", "IX<Esc>j.");
neovim_test!(dotrepeat, dot_append_line_end, "hello\nworld", "AX<Esc>j.");
neovim_test!(dotrepeat, dot_open_below, "line1\nline3", "oX<Esc>j.");
neovim_test!(dotrepeat, dot_open_above, "line2\nline3", "OX<Esc>j.");

// NEW: More basic dot repeat cases
neovim_test!(dotrepeat, dot_delete_X, "hello", "X.");
neovim_test!(dotrepeat, dot_delete_backward_word, "hello world test", "$db.");
neovim_test!(dotrepeat, dot_delete_e, "hello world", "de.");
neovim_test!(dotrepeat, dot_change_e, "hello world test", "ceX<Esc>w.");
neovim_test!(dotrepeat, dot_substitute, "abcdef", "sX<Esc>l.");
neovim_test!(dotrepeat, dot_substitute_line, "line1\nline2\nline3", "SX<Esc>j.");
neovim_test!(dotrepeat, dot_replace, "abcd", "rX.");
neovim_test!(dotrepeat, dot_multiple_inserts, "hello", "iXY<Esc>l.");
neovim_test!(dotrepeat, dot_insert_unicode, "hello", "i日本語<Esc>l.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT WITH COUNTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(dotrepeat, dot_count_override, "abcdef", "2x3.");
neovim_test!(dotrepeat, dot_delete_word_count, "a b c d e f", "dw2.");
neovim_test!(dotrepeat, dot_explicit_count_first, "abcdefgh", "3x.");

// NEW: More count edge cases
neovim_test!(dotrepeat, dot_preserves_count, "abcdefghij", "3x.");
neovim_test!(dotrepeat, dot_2x_then_5dot, "abcdefghijkl", "2x5.");
neovim_test!(dotrepeat, dot_3dw, "a b c d e f g", "3dw.");
neovim_test!(dotrepeat, dot_2dd, "l1\nl2\nl3\nl4\nl5\nl6", "2dd.");
neovim_test!(dotrepeat, dot_5s, "abcdefghij", "5sX<Esc>.");
neovim_test!(dotrepeat, dot_count_insert, "hello", "3iX<Esc>.");
neovim_test!(dotrepeat, dot_count_1, "abcd", "1x.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT WITH TEXT OBJECTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(dotrepeat, dot_delete_inner_word, "hello world foo bar", "diww.");
neovim_test!(dotrepeat, dot_change_inner_quotes, "\"hello\" \"world\"", "ci\"X<Esc>f\".");
neovim_test!(dotrepeat, dot_delete_inner_parens, "(a) (b) (c)", "di(f(.");

// NEW: More text object dot repeat cases
neovim_test!(dotrepeat, dot_delete_a_word, "hello world foo bar", "daww.");
neovim_test!(dotrepeat, dot_change_inner_braces, "{a} {b} {c}", cursor(0, 1), "ci{X<Esc>f{.");
neovim_test!(dotrepeat, dot_change_inner_bracket, "[a] [b] [c]", cursor(0, 1), "ci[X<Esc>f[.");
neovim_test!(dotrepeat, dot_delete_inner_tag, "<p>a</p><p>b</p>", cursor(0, 3), "ditfa.");
neovim_test!(dotrepeat, dot_change_a_word, "hello world foo", "cawX<Esc>.");
neovim_test!(dotrepeat, dot_yank_and_delete_iw, "hello world", "yiwwdiw.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT WITH MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(dotrepeat, dot_delete_find, "abcabc", "dfa.");
neovim_test!(dotrepeat, dot_delete_to_char, "abcabc", "dtaw.");

// NEW: More motion dot repeat cases
neovim_test!(dotrepeat, dot_delete_F, "abcabc", cursor(0, 5), "dFa0.");
neovim_test!(dotrepeat, dot_delete_T, "abcabc", cursor(0, 5), "dTa0.");
neovim_test!(dotrepeat, dot_delete_0, "hello\nworld", "d0j.");
neovim_test!(dotrepeat, dot_delete_dollar, "hello world\nfoo bar", "d$j.");
neovim_test!(dotrepeat, dot_delete_gg, "l1\nl2\nl3", cursor(2, 0), "dggG.");
neovim_test!(dotrepeat, dot_delete_percent, "(hello)\n(world)", "d%j.");
neovim_test!(dotrepeat, dot_change_find, "abcabc", "cfaX<Esc>.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT AFTER NON-REPEATABLE COMMANDS
// ═══════════════════════════════════════════════════════════════════════════════

// These should repeat the LAST change, not motion
neovim_test!(dotrepeat, dot_after_motion, "hello world", "xw.");
neovim_test!(dotrepeat, dot_after_undo, "abc", "xu.");

// NEW: More non-repeatable cases
neovim_test!(dotrepeat, dot_after_yank, "hello world", "xyw.");
neovim_test!(dotrepeat, dot_after_search, "hello hello", "x/hello<CR>.");
neovim_test!(dotrepeat, dot_after_mark, "hello", "xma.");
neovim_test!(dotrepeat, dot_after_jump, "hello world", "xfow.");
neovim_test!(dotrepeat, dot_after_gg_G, "l1\nl2\nl3", "xG.");
neovim_test!(dotrepeat, dot_after_scroll, "l1\nl2\nl3\nl4\nl5", "x<C-d>.");

// ═══════════════════════════════════════════════════════════════════════════════
// COMPLEX DOT SEQUENCES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(dotrepeat, dot_replace_and_repeat, "abcd", "rx.l.");
neovim_test!(dotrepeat, dot_multiple_deletes, "abcdefgh", "x...");

// NEW: More complex sequences
neovim_test!(dotrepeat, dot_insert_multiple, "hello world foo", "iX<Esc>w.w.");
neovim_test!(dotrepeat, dot_delete_multiple, "abcdefghijkl", "x....");
neovim_test!(dotrepeat, dot_cw_multiple, "one two three four", "cwX<Esc>w.w.");
neovim_test!(dotrepeat, dot_mixed_operations, "hello world", "xdwlsX<Esc>.");
neovim_test!(dotrepeat, dot_long_insert, "hello", "iABCDEFGHIJ<Esc>.");
neovim_test!(dotrepeat, dot_unicode_sequence, "日本語 テスト", "x.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT WITH VISUAL MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(dotrepeat, dot_visual_delete, "hello world foo", "vwd.");
neovim_test!(dotrepeat, dot_visual_change, "hello world foo", "vwcX<Esc>.");
neovim_test!(dotrepeat, dot_visual_line_delete, "l1\nl2\nl3\nl4", "Vjd.");
neovim_test!(dotrepeat, dot_visual_uppercase, "hello world foo", "vwgU.");
neovim_test!(dotrepeat, dot_visual_lowercase, "HELLO WORLD FOO", "vwgu.");
neovim_test!(dotrepeat, dot_visual_indent, "hello\nworld\nfoo", "Vj>.");
neovim_test!(dotrepeat, dot_visual_outdent, "    hello\n    world", "Vj<.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT WITH REGISTERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(dotrepeat, dot_delete_to_register, "hello world", "\"ax.");
neovim_test!(dotrepeat, dot_blackhole_delete, "hello world", "\"_dw.");
neovim_test!(dotrepeat, dot_yank_not_recorded, "hello world", "ywldw.");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(dotrepeat, dot_empty_buffer, "", ".");
neovim_test!(dotrepeat, dot_single_char_buffer, "a", "x.");
neovim_test!(dotrepeat, dot_at_eol, "hello", cursor(0, 4), "x.");
neovim_test!(dotrepeat, dot_at_bol, "hello", "x.");
neovim_test!(dotrepeat, dot_single_line, "hello world", "dw.");
neovim_test!(dotrepeat, dot_last_line, "l1\nl2", cursor(1, 0), "dd.");
neovim_test!(dotrepeat, dot_with_escape, "hello", "iX<Esc><Esc>l.");
neovim_test!(dotrepeat, dot_insert_then_delete, "hello", "iX<Esc>dw.");
neovim_test!(dotrepeat, dot_o_command, "hello", "oX<Esc>.");
neovim_test!(dotrepeat, dot_O_command, "hello", "OX<Esc>.");
neovim_test!(dotrepeat, dot_r_unicode, "hello", "r日.");
neovim_test!(dotrepeat, dot_cjk_content, "日本語テスト", "x.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — EXPANDED COVERAGE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(dotrepeat, dot_gJ, "a\nb\nc\nd", "gJ j.");
neovim_test!(dotrepeat, dot_replace_mode, "hello\nworld", "Rxy<Esc>j0.");
neovim_test!(dotrepeat, dot_indent, "hello\nworld", ">>j.");
neovim_test!(dotrepeat, dot_outdent, "    hello\n    world", "<<j.");
neovim_test!(dotrepeat, dot_tilde_count, "hello", "3~0.");

