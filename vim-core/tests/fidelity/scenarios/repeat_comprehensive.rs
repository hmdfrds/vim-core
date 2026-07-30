// Comprehensive Dot Repeat fidelity tests.
//
// Dot repeat with every repeatable command type, ensuring the last change
// is correctly recorded and replayed.

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — SINGLE CHARACTER OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_x_repeat, "abcdefgh", "x.");
neovim_test!(scenarios, dot_x_triple, "abcdefgh", "x..");
neovim_test!(scenarios, dot_X_repeat, "abcdefgh", cursor(0, 4), "X.");
neovim_test!(scenarios, dot_X_triple, "abcdefgh", cursor(0, 6), "X..");
neovim_test!(scenarios, dot_tilde_repeat, "abcdef", "~.");
neovim_test!(scenarios, dot_tilde_triple, "abcdef", "~..");
neovim_test!(scenarios, dot_r_repeat, "abcdef", "rX.");
neovim_test!(scenarios, rpt_dot_r_unicode, "abcdef", "r日.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — DELETE OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_dw_repeat, "one two three four five", "dw.");
neovim_test!(scenarios, dot_de_repeat, "one two three four five", "de.");
neovim_test!(scenarios, dot_db_repeat, "one two three four five", cursor(0, 18), "db.");
neovim_test!(scenarios, dot_dW_repeat, "foo.bar baz.qux end", "dW.");
neovim_test!(scenarios, dot_dE_repeat, "foo.bar baz.qux end", "dE.");
neovim_test!(scenarios, dot_dd_repeat, "l1\nl2\nl3\nl4", "dd.");
neovim_test!(scenarios, dot_D_repeat, "hello\nworld", "D.");
neovim_test!(scenarios, dot_dj_repeat, "l1\nl2\nl3\nl4\nl5", "dj.");
neovim_test!(scenarios, dot_dk_repeat, "l1\nl2\nl3\nl4\nl5", cursor(4, 0), "dk.");
neovim_test!(scenarios, dot_d0_repeat, "hello world\nfoo bar", cursor(0, 5), "d0j5l.");
neovim_test!(scenarios, dot_d_dollar_repeat, "hello world\nfoo bar", "d$j.");
neovim_test!(scenarios, dot_d_caret_repeat, "  hello\n  world", cursor(0, 6), "d^j6l.");
neovim_test!(scenarios, dot_df_repeat, "a;b;c;d;e", "df;.");
neovim_test!(scenarios, dot_dt_repeat, "a;b;c;d;e", "dt;.");
neovim_test!(scenarios, dot_dG_repeat, "l1\nl2\nl3\nl4\nl5", "dG");
neovim_test!(scenarios, dot_d_percent_repeat, "(a)(b)(c)", "d%.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — CHANGE OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_cw_repeat, "one two three four", "cwX<Esc>w.");
neovim_test!(scenarios, dot_ce_repeat, "one two three four", "ceX<Esc>w.");
neovim_test!(scenarios, dot_cb_repeat, "one two three four", cursor(0, 14), "cbX<Esc>b.");
neovim_test!(scenarios, dot_cc_repeat, "l1\nl2\nl3", "ccX<Esc>j.");
neovim_test!(scenarios, dot_C_repeat, "hello world\nfoo bar", "CX<Esc>j.");
neovim_test!(scenarios, dot_cj_repeat, "l1\nl2\nl3\nl4\nl5", "cjX<Esc>.");
neovim_test!(scenarios, dot_cf_repeat, "a;b;c;d", "cf;X<Esc>.");
neovim_test!(scenarios, dot_ct_repeat, "a;b;c;d", "ct;X<Esc>.");
neovim_test!(scenarios, dot_c_dollar_repeat, "hello world\nfoo bar", "c$X<Esc>j.");
neovim_test!(scenarios, dot_c_percent_repeat, "(hello)(world)", "c%X<Esc>.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — INSERT OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_i_repeat, "ab\ncd", "iX<Esc>j.");
neovim_test!(scenarios, dot_a_repeat, "ab\ncd", "aX<Esc>j.");
neovim_test!(scenarios, dot_I_repeat, "  hello\n  world", "IX<Esc>j.");
neovim_test!(scenarios, dot_A_repeat, "hello\nworld", "AX<Esc>j.");
neovim_test!(scenarios, dot_o_repeat, "l1\nl3", "oX<Esc>j.");
neovim_test!(scenarios, dot_O_repeat, "l2\nl4", "OX<Esc>jj.");
neovim_test!(scenarios, dot_s_repeat, "abcdef", "sX<Esc>l.");
neovim_test!(scenarios, dot_S_repeat, "l1\nl2\nl3", "SX<Esc>j.");
neovim_test!(scenarios, dot_i_multichar, "ab\ncd", "iXYZ<Esc>j.");
neovim_test!(scenarios, dot_a_multichar, "ab\ncd", "aXYZ<Esc>j.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — TEXT OBJECTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_diw_repeat, "one two three four", "diww.");
neovim_test!(scenarios, dot_daw_repeat, "one two three four", "daw.");
neovim_test!(scenarios, dot_ciw_repeat, "one two three four", "ciwX<Esc>w.");
neovim_test!(scenarios, dot_caw_repeat, "one two three four", "cawX<Esc>.");
neovim_test!(scenarios, dot_di_paren_repeat, "(a) (b) (c)", cursor(0, 1), "di(f(.");
neovim_test!(scenarios, dot_da_paren_repeat, "(a) (b) (c)", cursor(0, 1), "da(.");
neovim_test!(scenarios, dot_ci_paren_repeat, "(a) (b) (c)", cursor(0, 1), "ci(X<Esc>f(.");
neovim_test!(scenarios, dot_di_dquote_repeat, "\"a\" \"b\" \"c\"", cursor(0, 1), "di\"f\".");
neovim_test!(scenarios, dot_ci_dquote_repeat, "\"a\" \"b\" \"c\"", cursor(0, 1), "ci\"X<Esc>f\".");
neovim_test!(scenarios, dot_di_brace_repeat, "{a} {b} {c}", cursor(0, 1), "di{f{.");
neovim_test!(scenarios, dot_ci_bracket_repeat, "[a] [b] [c]", cursor(0, 1), "ci[X<Esc>f[.");
neovim_test!(scenarios, dot_dis_repeat, "One. Two. Three.", "dis.");
neovim_test!(scenarios, dot_dip_repeat, "p1\n\np2\n\np3", "dip.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — CASE OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_guw_repeat, "HELLO WORLD TEST", "guww.");
neovim_test!(scenarios, dot_gUw_repeat, "hello world test", "gUww.");
neovim_test!(scenarios, dot_g_tilde_w_repeat, "Hello World Test", "g~ww.");
neovim_test!(scenarios, dot_guu_repeat, "HELLO\nWORLD", "guuj.");
neovim_test!(scenarios, dot_gUU_repeat, "hello\nworld", "gUUj.");
neovim_test!(scenarios, dot_g_tilde_tilde_repeat, "Hello\nWorld", "g~~j.");
neovim_test!(scenarios, dot_guiw_repeat, "HELLO WORLD", "guiww.");
neovim_test!(scenarios, dot_gUiw_repeat, "hello world", "gUiww.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — INDENT/OUTDENT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_indent_line, "hello\nworld\ntest", ">>j.");
neovim_test!(scenarios, dot_outdent_line, "    hello\n    world\n    test", "<<j.");
neovim_test!(scenarios, dot_indent_2lines, "hello\nworld\ntest\nfour", ">jjj.");
neovim_test!(scenarios, dot_indent_ip, "hello\nworld\n\ntest\nfour", ">ipjjj.");
neovim_test!(scenarios, dot_double_indent, "hello", ">>.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — JOIN
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_J_repeat, "l1\nl2\nl3\nl4", "J.");
neovim_test!(scenarios, dot_gJ_repeat, "l1\nl2\nl3\nl4", "gJ.");
neovim_test!(scenarios, dot_J_triple, "l1\nl2\nl3\nl4", "J..");
neovim_test!(scenarios, dot_3J_repeat, "l1\nl2\nl3\nl4\nl5\nl6\nl7", "3J.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — REPLACE MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_R_repeat, "hello\nworld", "RXY<Esc>j0.");
neovim_test!(scenarios, dot_R_single, "hello\nworld", "RX<Esc>j0.");
neovim_test!(scenarios, dot_R_long, "hello world\nfoo bar baz", "RABC<Esc>j0.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — VISUAL MODE OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_vwd, "one two three four", "vwd.");
neovim_test!(scenarios, dot_ved, "one two three four", "ved.");
neovim_test!(scenarios, dot_vwc, "one two three four", "vwcX<Esc>.");
neovim_test!(scenarios, dot_V_delete, "l1\nl2\nl3\nl4", "Vd.");
neovim_test!(scenarios, dot_V_change, "l1\nl2\nl3\nl4", "VcX<Esc>.");
neovim_test!(scenarios, dot_Vj_delete, "l1\nl2\nl3\nl4\nl5", "Vjd.");
neovim_test!(scenarios, dot_v_gU, "hello world test four", "vwgU.");
neovim_test!(scenarios, dot_v_gu, "HELLO WORLD TEST FOUR", "vwgu.");
neovim_test!(scenarios, dot_V_indent, "hello\nworld\ntest\nfour", "V>j.");
neovim_test!(scenarios, dot_V_outdent, "    hello\n    world\n    test\n    four", "V<j.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — BLOCK VISUAL
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_ctrl_v_delete, "abc\ndef\nghi\njkl", "<C-v>jd.");
neovim_test!(scenarios, dot_ctrl_v_I, "abc\ndef\nghi", "<C-v>2jIX<Esc>.");
neovim_test!(scenarios, dot_ctrl_v_x, "abc\ndef\nghi\njkl", "<C-v>jx.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — PASTE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_p_charwise, "hello", "ywp.");
neovim_test!(scenarios, dot_p_linewise, "hello\nworld", "yyp.");
neovim_test!(scenarios, dot_P_charwise, "hello", "ywP.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — SEARCH THEN DOT
// ═══════════════════════════════════════════════════════════════════════════════

// cgn pattern: change next match, then dot to repeat
neovim_test!(scenarios, dot_cgn, "foo bar foo baz foo", "/foo<CR>cgnX<Esc>.");
neovim_test!(scenarios, dot_cgn_triple, "foo bar foo baz foo end foo", "/foo<CR>cgnX<Esc>..");
neovim_test!(scenarios, dot_dgn, "foo bar foo baz foo", "/foo<CR>dgn.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — AFTER NON-REPEATABLE
// ═══════════════════════════════════════════════════════════════════════════════

// Motion doesn't change last edit — dot still repeats previous change
neovim_test!(scenarios, dot_after_w, "abcdef", "xw.");
neovim_test!(scenarios, dot_after_gg, "l1\nl2", "xgg.");
neovim_test!(scenarios, dot_after_G, "l1\nl2", "xG.");
neovim_test!(scenarios, rpt_dot_after_search, "hello hello", "x/hello<CR>.");
neovim_test!(scenarios, rpt_dot_after_mark, "hello", "xma.");
neovim_test!(scenarios, rpt_dot_after_yank, "hello world", "xyw.");
neovim_test!(scenarios, rpt_dot_after_undo, "hello", "xu.");
neovim_test!(scenarios, dot_after_percent, "(hello)", "x%.");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_no_previous, "hello", ".");
neovim_test!(scenarios, rpt_dot_empty_buffer, "", ".");
neovim_test!(scenarios, dot_after_empty_insert, "hello", "i<Esc>.");
neovim_test!(scenarios, rpt_dot_at_eol, "hello", cursor(0, 4), "x.");
neovim_test!(scenarios, rpt_dot_at_bol, "hello", "x.");
neovim_test!(scenarios, dot_single_char, "a", "x.");
neovim_test!(scenarios, dot_unicode, "日本語テスト", "x.");
neovim_test!(scenarios, dot_emoji, "👍👍👍", "x.");
neovim_test!(scenarios, dot_five_times, "abcdefghij", "x.....");
neovim_test!(scenarios, dot_ten_times, "abcdefghijklmnop", "x.........");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT — COUNT INTERACTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_count_override_x, "abcdefgh", "2x3.");
neovim_test!(scenarios, dot_count_preserve_x, "abcdefgh", "3x.");
neovim_test!(scenarios, dot_count_override_dw, "a b c d e f g h", "dw2.");
neovim_test!(scenarios, dot_count_override_dd, "l1\nl2\nl3\nl4\nl5\nl6", "dd2.");
neovim_test!(scenarios, dot_count_override_cw, "one two three four five", "cwX<Esc>w2.");
