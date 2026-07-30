// Adversarial fidelity tests — designed to BREAK the Vim engine.
//
// These tests think like a fuzzer: they target state corruption, panics,
// hangs, off-by-one errors, and undefined behavior at the seams of the
// engine. Every test here is a plausible crash or corruption vector.

// ═══════════════════════════════════════════════════════════════════════════════
// RAPID MODE SWITCHING — state machine thrashing
// ═══════════════════════════════════════════════════════════════════════════════
// Rapidly entering and exiting modes stresses parser state, pending operator
// cleanup, and visual selection teardown.

neovim_test!(scenarios, adv_rapid_insert_escape, "hello", "i<Esc>i<Esc>i<Esc>i<Esc>i<Esc>");
neovim_test!(scenarios, adv_rapid_visual_escape, "hello", "v<Esc>v<Esc>v<Esc>v<Esc>v<Esc>");
neovim_test!(scenarios, adv_rapid_visual_line_escape, "hello\nworld", "V<Esc>V<Esc>V<Esc>V<Esc>");
neovim_test!(scenarios, adv_rapid_vblock_escape, "abc\ndef", "<C-v><Esc><C-v><Esc><C-v><Esc><C-v><Esc>");
neovim_test!(scenarios, adv_rapid_replace_escape, "hello", "R<Esc>R<Esc>R<Esc>R<Esc>");
neovim_test!(scenarios, adv_mode_thrash_insert_visual, "hello", "iv<Esc>iv<Esc>iv<Esc>");
neovim_test!(scenarios, adv_mode_thrash_all, "hello world", "iX<Esc>vld.VjdRab<Esc>");
neovim_test!(scenarios, adv_insert_char_rapid_exit, "abc", "iaiaia<Esc>");
neovim_test!(scenarios, adv_visual_toggle_storm, "hello\nworld", "vVv<C-v>Vv<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// NESTED UNDO GROUPS — undo tree corruption
// ═══════════════════════════════════════════════════════════════════════════════
// Interleaving insert/change with undo/redo can corrupt the undo tree or
// leave stale state in the redo stack.

neovim_test!(scenarios, adv_undo_redo_interleave, "hello", "iab<Esc>ucwXY<Esc>u<C-r>");
neovim_test!(scenarios, adv_undo_deep_then_redo_all, "abcdef", "xiX<Esc>dwcwZ<Esc>uuuu<C-r><C-r><C-r><C-r>");
neovim_test!(scenarios, adv_undo_after_dot, "hello world", "cwX<Esc>w.uu<C-r><C-r>");
neovim_test!(scenarios, adv_undo_insert_undo_insert, "hello", "iA<Esc>uiB<Esc>uiC<Esc>uu<C-r>");
neovim_test!(scenarios, adv_redo_past_new_edit, "abc", "xuiX<Esc>u<C-r><C-r><C-r>");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT OF NOTHING / DEGENERATE DOT
// ═══════════════════════════════════════════════════════════════════════════════
// Dot with no prior change, dot after failed command, dot after undo.

neovim_test!(scenarios, adv_dot_no_prior_change, "hello", ".");
neovim_test!(scenarios, adv_dot_after_motion_only, "hello world", "www.");
neovim_test!(scenarios, adv_dot_after_failed_search, "hello", "/zzz<CR>.");
neovim_test!(scenarios, adv_dot_after_undo, "hello", "xu.");
neovim_test!(scenarios, adv_dot_after_escape_insert, "hello", "i<Esc>.");

// ═══════════════════════════════════════════════════════════════════════════════
// MACRO EDGE CASES — empty macros, self-reference, recursion guard
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_empty_macro_play, "hello", "qaq@a");
neovim_test!(scenarios, adv_empty_macro_play_with_count, "hello", "qaq5@a");
neovim_test!(scenarios, adv_empty_macro_repeat, "hello", "qaq@a@@");
neovim_test!(scenarios, adv_macro_overwrite, "hello", "qaiX<Esc>qqaq@a");
neovim_test!(scenarios, adv_macro_play_unrecorded, "hello", "@z");
neovim_test!(scenarios, adv_macro_play_unrecorded_count, "hello", "99@z");
neovim_test!(scenarios, adv_macro_nested_playback, "ab\nab\nab", "qacwX<Esc>jqqb@aq@b");

// ═══════════════════════════════════════════════════════════════════════════════
// COMMANDS AT BUFFER BOUNDARIES — off-by-one / clamp failures
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_dollar_x, "hello", "$x");
neovim_test!(scenarios, adv_gg_0_x, "hello\nworld", "gg0x");
neovim_test!(scenarios, adv_G_dollar_x, "hello\nworld", "G$x");
neovim_test!(scenarios, adv_1G_dd, "hello\nworld\ntest", "1Gdd");
neovim_test!(scenarios, adv_G_dd, "hello\nworld", "Gdd");
neovim_test!(scenarios, adv_gg_dd, "only line", "ggdd");
neovim_test!(scenarios, adv_dollar_a_text, "hello", "$aXYZ<Esc>");
neovim_test!(scenarios, adv_0_i_text, "hello", "0iXYZ<Esc>");
neovim_test!(scenarios, adv_G_o, "hello\nworld", "Goappended<Esc>");
neovim_test!(scenarios, adv_gg_O, "hello\nworld", "ggOprepended<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL MODE EDGE CASES — selection + operation at extremes
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_v_dollar_d, "hello world", "v$d");
neovim_test!(scenarios, adv_V_dollar_d, "hello world", "V$d");
neovim_test!(scenarios, adv_vblock_dollar_d, "hello\nworld", "<C-v>j$d");
neovim_test!(scenarios, adv_v_gg_0_d, "hello\nworld\ntest", cursor(2, 3), "vgg0d");
neovim_test!(scenarios, adv_V_gg_d, "hello\nworld\ntest", cursor(2, 0), "Vggd");
neovim_test!(scenarios, adv_v_G_dollar_d, "hello\nworld\ntest", "vG$d");
neovim_test!(scenarios, adv_v_select_all_yank_paste, "hello\nworld", "ggvG$yGp");
neovim_test!(scenarios, adv_visual_empty_selection_d, "hello", "vd");
neovim_test!(scenarios, adv_V_single_line_dd, "only", "Vd");

// ═══════════════════════════════════════════════════════════════════════════════
// REPLACE AT BOUNDARIES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_r_at_eol, "hello", cursor(0, 4), "rx");
neovim_test!(scenarios, adv_r_at_bol, "hello", "rx");
neovim_test!(scenarios, adv_r_newline_at_eof, "hello", cursor(0, 4), "r<CR>");
neovim_test!(scenarios, adv_r_newline_at_bol, "hello", "r<CR>");
neovim_test!(scenarios, adv_r_on_empty, "", "rx");
neovim_test!(scenarios, adv_R_past_eol_long, "hi", "RABCDEF<Esc>");
neovim_test!(scenarios, adv_R_bs_past_original, "hi", "RABC<BS><BS><BS><BS>");

// ═══════════════════════════════════════════════════════════════════════════════
// DELETE MORE THAN EXISTS — count exceeding content
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_d99j_two_lines, "aa\nbb", "d99j");
neovim_test!(scenarios, adv_d99k_two_lines, "aa\nbb", cursor(1, 0), "d99k");
neovim_test!(scenarios, adv_d99w_short, "one two", "d99w");
neovim_test!(scenarios, adv_c99w_short, "one two", "c99wX<Esc>");
neovim_test!(scenarios, adv_999dd, "a\nb\nc", "999dd");
neovim_test!(scenarios, adv_999x, "hello", "999x");
neovim_test!(scenarios, adv_999X, "hello", cursor(0, 4), "999X");

// ═══════════════════════════════════════════════════════════════════════════════
// YANK/PASTE CYCLES — register state after undo/redo
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_yy_3p_u, "hello", "yy3pu");
neovim_test!(scenarios, adv_yy_3p_uu, "hello", "yy3puu");
neovim_test!(scenarios, adv_yy_3p_uuu, "hello", "yy3puuu");
neovim_test!(scenarios, adv_yy_3p_u_ctrl_r, "hello", "yy3pu<C-r>");
neovim_test!(scenarios, adv_yw_p_dot_dot, "hello world", "ywp..");
neovim_test!(scenarios, adv_dd_p_u_p, "hello\nworld\ntest", "ddpu");
neovim_test!(scenarios, adv_xp_on_single_char, "a", "xp");
neovim_test!(scenarios, adv_ddp_on_single_line, "only", "ddp");

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT MODE CHAOS — backspace beyond content, special keys
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_insert_bs_more_than_chars, "ab", "A<BS><BS><BS><BS>");
neovim_test!(scenarios, adv_insert_bs_on_empty, "", "i<BS><BS><BS>");
neovim_test!(scenarios, adv_insert_bs_across_line, "hello\nworld", cursor(1, 0), "i<BS>");
neovim_test!(scenarios, adv_insert_ctrl_w_on_empty, "", "i<C-w><C-w>");
neovim_test!(scenarios, adv_insert_ctrl_u_on_empty, "", "i<C-u><C-u>");
neovim_test!(scenarios, adv_insert_ctrl_w_single_word, "hello", "A<C-w><C-w>");
neovim_test!(scenarios, adv_insert_ctrl_u_full_line, "hello world", "A<C-u><C-u>");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNT OVERFLOW — absurd counts on short content
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_99999l_short, "hello", "99999l");
neovim_test!(scenarios, adv_99999h_short, "hello", cursor(0, 4), "99999h");
neovim_test!(scenarios, adv_99999j_short, "a\nb", "99999j");
neovim_test!(scenarios, adv_99999k_short, "a\nb", cursor(1, 0), "99999k");
neovim_test!(scenarios, adv_99999w_short, "hi", "99999w");
neovim_test!(scenarios, adv_99999x_single, "a", "99999x");
neovim_test!(scenarios, adv_99999p_empty_reg, "hello", "99999p");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL BLOCK ON SINGLE COLUMN / DEGENERATE GEOMETRY
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_vblock_single_col_delete, "a\nb\nc\nd", "<C-v>3jd");
neovim_test!(scenarios, adv_vblock_single_col_change, "a\nb\nc\nd", "<C-v>3jcX<Esc>");
neovim_test!(scenarios, adv_vblock_single_col_yank_paste, "a\nb\nc\nd", "<C-v>3jyjp");
neovim_test!(scenarios, adv_vblock_single_cell, "x", "<C-v>d");
neovim_test!(scenarios, adv_vblock_single_row, "hello", "<C-v>3ld");
neovim_test!(scenarios, adv_vblock_zero_width_dollar, "hi\n\nhi", "<C-v>2j$d");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH FOR NON-EXISTENT — error handling in search/n/N
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_search_nonexistent, "hello world", "/zzzzz<CR>");
neovim_test!(scenarios, adv_search_nonexistent_n, "hello world", "/zzzzz<CR>n");
neovim_test!(scenarios, adv_search_nonexistent_N, "hello world", "/zzzzz<CR>N");
neovim_test!(scenarios, adv_search_nonexistent_then_edit, "hello world", "/zzzzz<CR>x");
neovim_test!(scenarios, adv_n_without_prior_search, "hello", "n");
neovim_test!(scenarios, adv_N_without_prior_search, "hello", "N");
neovim_test!(scenarios, adv_star_nonexistent_word, " ", "*");
neovim_test!(scenarios, adv_hash_nonexistent_word, " ", "#");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATOR PENDING CANCELLATION — start operator then bail out
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_d_escape, "hello world", "d<Esc>");
neovim_test!(scenarios, adv_c_escape, "hello world", "c<Esc>");
neovim_test!(scenarios, adv_y_escape, "hello world", "y<Esc>");
neovim_test!(scenarios, adv_gU_escape, "hello world", "gU<Esc>");
neovim_test!(scenarios, adv_gu_escape, "hello world", "gu<Esc>");
neovim_test!(scenarios, adv_g_tilde_escape, "hello world", "g~<Esc>");
neovim_test!(scenarios, adv_d_then_d_escape, "hello\nworld", "dd<Esc>");
neovim_test!(scenarios, adv_operator_pending_then_mode_switch, "hello", "dv<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// TWO-LINE BUFFER STRESS — many operations on minimal multi-line doc
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_two_line_J, "a\nb", "J");
neovim_test!(scenarios, adv_two_line_gJ, "a\nb", "gJ");
neovim_test!(scenarios, adv_two_line_dd_first, "a\nb", "dd");
neovim_test!(scenarios, adv_two_line_dd_second, "a\nb", cursor(1, 0), "dd");
neovim_test!(scenarios, adv_two_line_dG, "a\nb", "dG");
neovim_test!(scenarios, adv_two_line_dgg, "a\nb", cursor(1, 0), "dgg");
neovim_test!(scenarios, adv_two_line_VGd, "a\nb", "VGd");
neovim_test!(scenarios, adv_two_line_yy_p, "a\nb", "yyp");
neovim_test!(scenarios, adv_two_line_ddp, "a\nb", "ddp");
neovim_test!(scenarios, adv_two_line_xp, "ab", "xp");

// ═══════════════════════════════════════════════════════════════════════════════
// WHITESPACE-ONLY AND BLANK-LINE DOCUMENTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_spaces_dd, "   ", "dd");
neovim_test!(scenarios, adv_spaces_dw, "   ", "dw");
neovim_test!(scenarios, adv_spaces_cw, "   ", "cwX<Esc>");
neovim_test!(scenarios, adv_spaces_diw, "   ", "diw");
neovim_test!(scenarios, adv_spaces_x, "   ", "x");
neovim_test!(scenarios, adv_blank_lines_dd, "\n\n\n", "dd");
neovim_test!(scenarios, adv_blank_lines_dj, "\n\n\n", "dj");
neovim_test!(scenarios, adv_blank_lines_J, "\n\n\n", "J");
neovim_test!(scenarios, adv_blank_lines_p_after_yy, "\n\n\n", "yyp");
neovim_test!(scenarios, adv_tabs_only, "\t\t\t", "x");

// ═══════════════════════════════════════════════════════════════════════════════
// COMBINED MULTI-STEP ADVERSARIAL SEQUENCES
// ═══════════════════════════════════════════════════════════════════════════════
// Long sequences that exercise many subsystems in one test.

neovim_test!(scenarios, adv_delete_undo_visual_paste, "hello world test", "dwiXY<Esc>uvwdp");
neovim_test!(scenarios, adv_macro_undo_redo, "abc\ndef\nghi", "qacwX<Esc>jq@au<C-r>");
neovim_test!(scenarios, adv_search_delete_undo_redo, "foo bar foo baz", "/foo<CR>dwu<C-r>n.");
neovim_test!(scenarios, adv_visual_block_insert_undo, "aaa\nbbb\nccc", "<C-v>2jI#<Esc>u<C-r>");
neovim_test!(scenarios, adv_replace_mode_undo_dot, "hello world", "Rabc<Esc>u..");
neovim_test!(scenarios, adv_indent_undo_dot, "hello\nworld\ntest", ">>j>>j>>uuu...");
neovim_test!(scenarios, adv_mark_delete_undo_jump, "hello\nworld\ntest", "jmaddkdd2u'a");
neovim_test!(scenarios, adv_yank_change_paste_undo, "hello world foo bar", "ywwcwPASTED<Esc>u\"0p");

// ═══════════════════════════════════════════════════════════════════════════════
// gU / gu / g~ ON EMPTY/DEGENERATE BUFFERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_gU_on_empty, "", "gUU");
neovim_test!(scenarios, adv_gu_on_empty, "", "guu");
neovim_test!(scenarios, adv_g_tilde_on_empty, "", "g~~");
neovim_test!(scenarios, adv_gU_w_single_char, "a", "gUw");
neovim_test!(scenarios, adv_gu_w_single_char, "A", "guw");
neovim_test!(scenarios, adv_gU_dollar_at_eol, "hello", cursor(0, 4), "gU$");
neovim_test!(scenarios, adv_gu_0_at_bol, "HELLO", "gu0");
neovim_test!(scenarios, adv_g_tilde_G, "Hello\nWorld", "g~G");
neovim_test!(scenarios, adv_gUgU_alias, "hello", "gUgU");

// ═══════════════════════════════════════════════════════════════════════════════
// EX COMMANDS ON EDGE BUFFERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, adv_ex_sub_no_match, "hello", ":s/zzz/xxx/<CR>");
neovim_test!(scenarios, adv_ex_sub_empty_buffer, "", ":s/a/b/<CR>");
neovim_test!(scenarios, adv_ex_delete_all, "hello\nworld", ":%d<CR>");
neovim_test!(scenarios, adv_ex_global_no_match, "hello", ":g/zzz/d<CR>");
neovim_test!(scenarios, adv_ex_norm_on_empty, "", ":%norm A!<CR>");
neovim_test!(scenarios, adv_ex_join_single_line, "hello", ":j<CR>");
