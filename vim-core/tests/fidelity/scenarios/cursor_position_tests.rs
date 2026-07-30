// Cursor Position Verification fidelity tests.
//
// Systematic tests verifying cursor position after operations.
// Many bugs manifest as correct text but wrong cursor position.

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER DELETE
// ═══════════════════════════════════════════════════════════════════════════════

// After dw, cursor stays at position
neovim_test!(scenarios, cursor_after_dw, "hello world test", "dw");
neovim_test!(scenarios, cursor_after_dw_mid, "hello world test", cursor(0, 6), "dw");
neovim_test!(scenarios, cursor_after_dw_last, "hello world", cursor(0, 6), "dw");
neovim_test!(scenarios, cursor_after_de, "hello world test", "de");
neovim_test!(scenarios, cursor_after_db, "hello world test", cursor(0, 6), "db");

// After dd, cursor goes to first non-blank of next line
neovim_test!(scenarios, cursor_after_dd_first, "  hello\n  world\n  test", "dd");
neovim_test!(scenarios, cursor_after_dd_mid, "  hello\n  world\n  test", cursor(1, 0), "dd");
neovim_test!(scenarios, cursor_after_dd_last, "  hello\n  world\n  test", cursor(2, 0), "dd");
neovim_test!(scenarios, cursor_after_dd_only, "hello", "dd");

// After x, cursor doesn't go past EOL
neovim_test!(scenarios, cursor_after_x_end, "hello", cursor(0, 4), "x");
neovim_test!(scenarios, cursor_after_x_mid, "hello", cursor(0, 2), "x");
neovim_test!(scenarios, cursor_after_x_single, "a", "x");

// After D, cursor at last char of remaining
neovim_test!(scenarios, cursor_after_D_mid, "hello world", cursor(0, 5), "D");
neovim_test!(scenarios, cursor_after_D_start, "hello world", "D");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER CHANGE
// ═══════════════════════════════════════════════════════════════════════════════

// After cw + Esc, cursor on last typed char
neovim_test!(scenarios, cursor_after_cw, "hello world", "cwX<Esc>");
neovim_test!(scenarios, cursor_after_cw_multi, "hello world", "cwXYZ<Esc>");
neovim_test!(scenarios, cursor_after_cc, "hello world", "ccX<Esc>");
neovim_test!(scenarios, cursor_after_C, "hello world", cursor(0, 5), "CX<Esc>");
neovim_test!(scenarios, cursor_after_ce, "hello world", "ceX<Esc>");
neovim_test!(scenarios, cursor_after_ciw, "hello world", cursor(0, 2), "ciwX<Esc>");
neovim_test!(scenarios, cursor_after_ci_paren, "(hello)", cursor(0, 3), "ci(X<Esc>");
neovim_test!(scenarios, cursor_after_ci_dquote, "\"hello\"", cursor(0, 3), "ci\"X<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER YANK
// ═══════════════════════════════════════════════════════════════════════════════

// After yw, cursor stays at start
neovim_test!(scenarios, cursor_after_yw, "hello world", "yw");
neovim_test!(scenarios, cursor_after_ye, "hello world", "ye");
neovim_test!(scenarios, cursor_after_yy, "hello world", "yy");
neovim_test!(scenarios, cursor_after_y_dollar, "hello world", "y$");
neovim_test!(scenarios, cursor_after_yiw, "hello world", cursor(0, 2), "yiw");
neovim_test!(scenarios, cursor_after_yi_paren, "(hello)", cursor(0, 3), "yi(");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER PASTE
// ═══════════════════════════════════════════════════════════════════════════════

// p charwise: cursor on last pasted char
neovim_test!(scenarios, cursor_after_p_char, "hello", "ywp");
neovim_test!(scenarios, cursor_after_p_char_end, "hello", "yw$p");
neovim_test!(scenarios, cursor_after_P_char, "hello", "ywP");

// p linewise: cursor on first non-blank of pasted line
neovim_test!(scenarios, cursor_after_p_line, "  hello\n  world", "yyp");
neovim_test!(scenarios, cursor_after_P_line, "  hello\n  world", cursor(1, 0), "yyP");

// After dd then p
neovim_test!(scenarios, cursor_after_ddp, "line1\nline2\nline3", cursor(1, 0), "ddp");
neovim_test!(scenarios, cursor_after_ddP, "line1\nline2\nline3", cursor(1, 0), "ddP");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER INSERT MODE EXIT
// ═══════════════════════════════════════════════════════════════════════════════

// After i...Esc, cursor moves left one
neovim_test!(scenarios, cursor_after_i_esc, "hello", "iX<Esc>");
neovim_test!(scenarios, cursor_after_i_esc_empty, "", "iX<Esc>");
neovim_test!(scenarios, cursor_after_i_esc_multi, "hello", "iXYZ<Esc>");
neovim_test!(scenarios, cursor_after_a_esc, "hello", "aX<Esc>");
neovim_test!(scenarios, cursor_after_A_esc, "hello", "AX<Esc>");
neovim_test!(scenarios, cursor_after_I_esc, "  hello", "IX<Esc>");
neovim_test!(scenarios, cursor_after_o_esc, "hello", "oX<Esc>");
neovim_test!(scenarios, cursor_after_O_esc, "hello", "OX<Esc>");
neovim_test!(scenarios, cursor_after_s_esc, "hello", "sX<Esc>");
neovim_test!(scenarios, cursor_after_S_esc, "hello", "SX<Esc>");

// Empty insert (no text typed)
neovim_test!(scenarios, cursor_after_i_esc_notype, "hello", cursor(0, 2), "i<Esc>");
neovim_test!(scenarios, cursor_after_a_esc_notype, "hello", cursor(0, 2), "a<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER UNDO
// ═══════════════════════════════════════════════════════════════════════════════

// Undo restores cursor to where the change was
neovim_test!(scenarios, cursor_after_undo_x, "hello", cursor(0, 2), "xu");
neovim_test!(scenarios, cursor_after_undo_dw, "hello world", "dwu");
neovim_test!(scenarios, cursor_after_undo_dd, "hello\nworld", "ddu");
neovim_test!(scenarios, cursor_after_undo_cw, "hello world", "cwX<Esc>u");
neovim_test!(scenarios, cursor_after_undo_insert, "hello", "iX<Esc>u");
neovim_test!(scenarios, cursor_after_redo, "hello", "xu<C-r>");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER JOIN
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cursor_after_J, "hello\nworld", "J");
neovim_test!(scenarios, cursor_after_gJ, "hello\nworld", "gJ");
neovim_test!(scenarios, cursor_after_J_indented, "hello\n  world", "J");
neovim_test!(scenarios, cursor_after_2J, "l1\nl2\nl3", "2J");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER CASE CHANGE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cursor_after_guw, "HELLO WORLD", "guw");
neovim_test!(scenarios, cursor_after_gUw, "hello world", "gUw");
neovim_test!(scenarios, cursor_after_guu, "HELLO WORLD", "guu");
neovim_test!(scenarios, cursor_after_gUU, "hello world", "gUU");
neovim_test!(scenarios, cursor_after_tilde, "hello", "~");
neovim_test!(scenarios, cursor_after_3_tilde, "hello", "3~");
neovim_test!(scenarios, cursor_after_g_tilde_w, "HeLLo WoRLd", "g~w");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER INDENT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cursor_after_indent, "hello", ">>");
neovim_test!(scenarios, cursor_after_outdent, "    hello", "<<");
neovim_test!(scenarios, cursor_after_indent_j, "hello\nworld", ">j");
neovim_test!(scenarios, cursor_after_eq_eq, "  hello", "==");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER REPLACE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cursor_after_r, "hello", "rx");
neovim_test!(scenarios, cursor_after_r_mid, "hello", cursor(0, 2), "rx");
neovim_test!(scenarios, cursor_after_r_end, "hello", cursor(0, 4), "rx");
neovim_test!(scenarios, cursor_after_3r, "hello", "3rx");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER REPLACE MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cursor_after_R_esc, "hello", "RXY<Esc>");
neovim_test!(scenarios, cursor_after_R_esc_single, "hello", "RX<Esc>");
neovim_test!(scenarios, cursor_after_R_esc_end, "hello", cursor(0, 3), "RXY<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER SEARCH
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cursor_after_search_fwd, "hello world hello", "/hello<CR>");
neovim_test!(scenarios, cursor_after_search_bwd, "hello world hello", cursor(0, 12), "?hello<CR>");
neovim_test!(scenarios, cursor_after_n, "hello world hello", "/hello<CR>n");
neovim_test!(scenarios, cursor_after_N, "hello world hello", "/hello<CR>N");
neovim_test!(scenarios, cursor_after_star, "hello world hello", "*");
neovim_test!(scenarios, cursor_after_hash, "hello world hello", cursor(0, 12), "#");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER MARK JUMP
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cursor_after_mark_backtick, "hello world", cursor(0, 5), "ma0`a");
neovim_test!(scenarios, cursor_after_mark_quote, "  hello\n  world", cursor(0, 4), "maG'a");
neovim_test!(scenarios, cursor_after_mark_backtick_line, "hello\nworld\ntest", cursor(1, 3), "magg`a");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER VISUAL MODE EXIT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cursor_after_v_esc, "hello world", "vw<Esc>");
neovim_test!(scenarios, cursor_after_v_esc_back, "hello world", cursor(0, 10), "v0<Esc>");
neovim_test!(scenarios, cursor_after_V_esc, "hello\nworld", "Vj<Esc>");
neovim_test!(scenarios, cursor_after_ctrl_v_esc, "abc\ndef", "<C-v>jl<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER DOT REPEAT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cursor_after_dot_x, "abcdef", "x.");
neovim_test!(scenarios, cursor_after_dot_dw, "one two three four", "dw.");
neovim_test!(scenarios, cursor_after_dot_cw, "one two three four", "cwX<Esc>w.");
neovim_test!(scenarios, cursor_after_dot_dd, "l1\nl2\nl3\nl4", "dd.");
neovim_test!(scenarios, cursor_after_dot_i, "ab\ncd", "iX<Esc>j.");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR POSITION AFTER gi
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cursor_gi_basic, "hello world", "iX<Esc>wgi");
neovim_test!(scenarios, cursor_gi_after_A, "hello\nworld", "AX<Esc>jgi");
neovim_test!(scenarios, cursor_gi_after_o, "hello\nworld", "oX<Esc>kgi");
neovim_test!(scenarios, cursor_gi_after_I, "  hello", "IX<Esc>$gi");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER MACRO PLAYBACK
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cursor_after_macro_x, "abcdef", "qaxq@a");
neovim_test!(scenarios, cursor_after_macro_dw, "one two three", "qadwq@a");
neovim_test!(scenarios, cursor_after_macro_motion, "hello world", "qalq0@a");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR AFTER EX COMMANDS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cursor_after_ex_d, "l1\nl2\nl3", ":d<CR>");
neovim_test!(scenarios, cursor_after_ex_d_2, "l1\nl2\nl3", ":2d<CR>");
neovim_test!(scenarios, cursor_after_ex_sub, "hello world", ":s/hello/bye<CR>");
neovim_test!(scenarios, cursor_after_ex_goto, "l1\nl2\nl3", ":2<CR>");
neovim_test!(scenarios, cursor_after_ex_goto_last, "l1\nl2\nl3", ":$<CR>");
neovim_test!(scenarios, cursor_after_ex_move, "l1\nl2\nl3", ":1m3<CR>");
neovim_test!(scenarios, cursor_after_ex_copy, "l1\nl2\nl3", ":1co3<CR>");
