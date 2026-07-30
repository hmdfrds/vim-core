// Undo/Redo fidelity tests for vim-core.
//
// Tests for `u` (undo) and `<C-r>` (redo) commands.
// Undo/redo is fundamental to safe editing in Vim.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC UNDO
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(undo, undo_delete_char, "hello", "xu");
neovim_test!(undo, undo_delete_word, "hello world", "dwu");
neovim_test!(undo, undo_delete_line, "line1\nline2", "ddu");
neovim_test!(undo, undo_change_word, "hello", "cwX<Esc>u");
neovim_test!(undo, undo_insert, "hello", "iX<Esc>u");

// NEW: More basic undo cases
neovim_test!(undo, undo_X, "hello", "lXu");
neovim_test!(undo, undo_D, "hello world", cursor(0, 6), "Du");
neovim_test!(undo, undo_C, "hello world", cursor(0, 6), "CX<Esc>u");
neovim_test!(undo, undo_s, "hello", "sX<Esc>u");
neovim_test!(undo, undo_S, "line1\nline2", "SX<Esc>u");
neovim_test!(undo, undo_r, "hello", "rXu");
neovim_test!(undo, undo_J, "hello\nworld", "Ju");
neovim_test!(undo, undo_o, "hello", "oWorld<Esc>u");
neovim_test!(undo, undo_O, "hello", "OWorld<Esc>u");
neovim_test!(undo, undo_append, "hello", "aX<Esc>u");
neovim_test!(undo, undo_A, "hello", "AX<Esc>u");
neovim_test!(undo, undo_I, "hello", "IX<Esc>u");
neovim_test!(undo, undo_unicode_insert, "hello", "i日本語<Esc>u");
neovim_test!(undo, undo_unicode_delete, "日本語", "xu");

// ═══════════════════════════════════════════════════════════════════════════════
// MULTIPLE UNDO
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(undo, undo_multiple, "abc", "xxxuu");
neovim_test!(undo, undo_with_count, "abcd", "xxx2u");
neovim_test!(undo, undo_all, "abcd", "xxxxuuuu");

// NEW: More multiple undo cases
neovim_test!(undo, undo_3_changes, "abcdef", "xdwsX<Esc>uuu");
neovim_test!(undo, undo_mixed_ops, "hello world", "xdwcwX<Esc>uuu");
neovim_test!(undo, undo_count_3, "abcdef", "xxx3u");
neovim_test!(undo, undo_count_5, "abcdefgh", "xxxxx5u");
neovim_test!(undo, undo_exceeds_history, "abc", "xuuuuuu");
neovim_test!(undo, undo_insert_multiple, "hello", "iA<Esc>iB<Esc>iC<Esc>uuu");

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC REDO
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(undo, redo_single, "abc", "xu<C-r>");
neovim_test!(undo, redo_after_multiple_undo, "abc", "xxuu<C-r>");
neovim_test!(undo, redo_with_count, "abcd", "xxxuuu2<C-r>");

// NEW: More redo cases
neovim_test!(undo, redo_all, "abc", "xxuuu<C-r><C-r>");
neovim_test!(undo, redo_count_3, "abcdef", "xxxxuuuu3<C-r>");
neovim_test!(undo, redo_exceeds_stack, "abc", "xu<C-r><C-r><C-r>");
neovim_test!(undo, redo_after_undo_word, "hello world", "dwu<C-r>");
neovim_test!(undo, redo_after_undo_line, "line1\nline2", "ddu<C-r>");
neovim_test!(undo, redo_insert, "hello", "iX<Esc>u<C-r>");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO/REDO INTERACTION
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(undo, undo_redo_cycle, "abc", "xu<C-r>u");
neovim_test!(undo, new_change_clears_redo, "abc", "xuxu");

// NEW: More undo/redo interaction cases
neovim_test!(undo, change_after_undo_clears_redo, "abcd", "xxuu<C-r>xu");
neovim_test!(undo, undo_redo_undo, "abc", "xu<C-r>u<C-r>");
neovim_test!(undo, multiple_undo_redo_cycle, "abcd", "xxxuuu<C-r><C-r><C-r>");
neovim_test!(undo, undo_then_insert_clears_redo, "hello", "xuiX<Esc>u");
neovim_test!(undo, redo_partial, "abcdef", "xxxxuuuu<C-r><C-r>");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO ACROSS LINES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(undo, undo_join_lines, "a\nb", "Ju");
neovim_test!(undo, undo_open_line, "a", "oX<Esc>u");
neovim_test!(undo, undo_multiline_delete, "a\nb\nc", "2ddu");

// NEW: More multiline undo cases
neovim_test!(undo, undo_dd_multiple, "l1\nl2\nl3\nl4", "ddjdduu");
neovim_test!(undo, undo_yy_p, "hello\nworld", "yyPu");
neovim_test!(undo, undo_visual_line_delete_basic, "l1\nl2\nl3", "Vjdu");
neovim_test!(undo, undo_multiple_joins, "a\nb\nc", "JJuu");
neovim_test!(undo, undo_o_multiple, "hello", "oA<Esc>oB<Esc>uu");
neovim_test!(undo, undo_O_then_o, "hello", "OA<Esc>oB<Esc>uu");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO WITH VISUAL MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(undo, undo_visual_delete, "hello world", "vwdu");
neovim_test!(undo, undo_visual_change, "hello world", "vwcX<Esc>u");
neovim_test!(undo, undo_visual_line_delete, "line1\nline2\nline3", "Vjdu");
neovim_test!(undo, undo_visual_uppercase, "hello", "vwgUu");
neovim_test!(undo, undo_visual_lowercase, "HELLO", "vwguu");
neovim_test!(undo, undo_visual_indent, "hello", "V>u");
neovim_test!(undo, undo_visual_outdent, "    hello", "V<u");
neovim_test!(undo, undo_block_visual, "abc\ndef", "<C-v>jxu");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO WITH TEXT OBJECTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(undo, undo_diw, "hello world", "diwu");
neovim_test!(undo, undo_ciw, "hello world", "ciwX<Esc>u");
neovim_test!(undo, undo_di_paren, "(hello)", cursor(0, 3), "di(u");
neovim_test!(undo, undo_ci_quote, "\"hello\"", cursor(0, 3), "ci\"X<Esc>u");
neovim_test!(undo, undo_dap, "hello\n\nworld", "dapu");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO WITH REGISTERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(undo, undo_named_reg_delete, "hello", "\"axu");
neovim_test!(undo, undo_blackhole_delete, "hello", "\"_xu");
neovim_test!(undo, undo_preserves_yanked, "hello world", "yiwdwu");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(undo, undo_at_start, "abc", "u");
neovim_test!(undo, redo_at_end, "abc", "<C-r>");

// NEW: More edge cases
neovim_test!(undo, undo_empty_buffer, "", "u");
neovim_test!(undo, redo_empty_buffer, "", "<C-r>");
neovim_test!(undo, undo_single_char, "a", "xu");
neovim_test!(undo, undo_at_eol, "hello", cursor(0, 4), "xu");
neovim_test!(undo, undo_at_bol, "hello", "xu");
neovim_test!(undo, undo_unicode_cjk, "日本語テスト", "xu");
neovim_test!(undo, undo_emoji, "👍 test", "xu");
neovim_test!(undo, undo_after_motion, "hello world", "xwu");
neovim_test!(undo, undo_after_search, "hello", "x/hello<CR>u");
neovim_test!(undo, undo_long_insert, "hello", "iABCDEFGHIJ<Esc>u");
neovim_test!(undo, undo_replace_mode, "hello", "RXYZ<Esc>u");
neovim_test!(undo, undo_dot_command, "hello", "x.u");
neovim_test!(undo, undo_count_exceeds, "abcd", "xx99u");
neovim_test!(undo, redo_count_exceeds, "abcd", "xxuu99<C-r>");

// ─────────────────────────────────────────────────────────────────────────────
// Undo Edge Cases
// ─────────────────────────────────────────────────────────────────────────────

// Undo after dot repeat
neovim_test!(undo, undo_after_dot, "aaa bbb ccc", "ciwXX<Esc>w.u");
// Undo restores correct cursor position
neovim_test!(undo, undo_cursor_pos, "hello world", "wcwfoo<Esc>u");
// Redo then new edit clears redo stack
neovim_test!(undo, redo_cleared_by_edit, "hello", "cwfoo<Esc>ucwbar<Esc><C-r>");
// Multiple undo past beginning
neovim_test!(undo, undo_past_beginning, "hello", "cwfoo<Esc>uuuuu");
// Undo after substitute
neovim_test!(undo, undo_after_sub_global, "aaa bbb aaa", ":s/aaa/xxx/g<CR>u");
// Undo visual block operation
neovim_test!(undo, undo_vblock_insert, "aaa\nbbb\nccc", "<C-v>2jI# <Esc>u");
// Redo visual block operation
neovim_test!(undo, redo_vblock_insert, "aaa\nbbb\nccc", "<C-v>2jI# <Esc>u<C-r>");
// Undo after gU (uppercase)
neovim_test!(undo, undo_gU_word, "hello world", "gUwu");
// Undo after >> (indent)
neovim_test!(undo, undo_indent_line, "hello", ">>u");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO LINE (U)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(undo, undo_line_basic, "hello world", "dwU");
neovim_test!(undo, undo_line_multiple_changes, "hello world", "dwxU");
neovim_test!(undo, undo_line_after_move, "line1\nline2", "ddjU");
neovim_test!(undo, undo_line_no_changes, "hello", "U");
neovim_test!(undo, undo_line_insert, "hello", "ciwnew<Esc>U");

// ═══════════════════════════════════════════════════════════════════════════════
// :EARLIER / :LATER (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// :earlier 1f parses f suffix without error (no-op when no save history)
neovim_test!(undo, earlier_1f_parse, "hello", ":earlier 1f<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// CONSECUTIVE UNDO ACROSS EDIT TYPES
// ═══════════════════════════════════════════════════════════════════════════════

// Two ciw edits on different words, consecutive undo restores both
neovim_test!(undo, undo_two_ciw_consecutive, "aaa bbb ccc", "ciwXXX<Esc>wciwYYY<Esc>uu");
// ciw + insert on different lines, consecutive undo
neovim_test!(undo, undo_ciw_then_insert_different_line, "aaa bbb\nccc ddd", "ciwXXX<Esc>jiwYYY<Esc>uu");
// Two edits with cursor motion between, undo both
neovim_test!(undo, undo_edit_motion_edit, "aaa bbb\nccc ddd", "ciwXXX<Esc>jciwYYY<Esc>uu");
// ciw + ea insert, consecutive undo (original bug scenario variant)
neovim_test!(undo, undo_ciw_then_append, "aaa bbb\nccc ddd", "ciwXXX<Esc>eaYYY<Esc>uu");
// Three edits, three undos
neovim_test!(undo, undo_three_edits_consecutive, "aaa bbb ccc", "ciwXXX<Esc>wciwYYY<Esc>wciwZZZ<Esc>uuu");

