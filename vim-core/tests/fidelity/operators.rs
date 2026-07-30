// Operator fidelity tests for vim-core.
//
// These tests compare vim-core operator output against Neovim oracle.
// 60 tests covering all operator categories per phase-6-operators.md.
//
// Test Categories:
// - Delete (d): 15 tests
// - Yank (y): 10 tests
// - Change (c): 10 tests
// - Indent (>, <): 8 tests
// - Case (gu, gU, g~): 10 tests
// - Format (gq): 5 tests
// - Combined (operator + motion): 12 tests

// ═══════════════════════════════════════════════════════════════════════════════
// DELETE OPERATOR (d)
// ═══════════════════════════════════════════════════════════════════════════════

// d{motion} - basic delete with motion
neovim_test!(operators, dw_word, "hello world", "dw");
neovim_test!(operators, de_word_end, "hello world", "de");
neovim_test!(operators, d_dollar_to_eol, "hello world", "d$");
neovim_test!(operators, d_zero_to_bol, "hello world", cursor(0, 5), "d0");
neovim_test!(operators, dh_left, "hello", cursor(0, 2), "dh");
neovim_test!(operators, dl_right, "hello", "dl");

// dd - delete line
neovim_test!(operators, dd_single, "line1\nline2\nline3", cursor(1, 0), "dd");
neovim_test!(operators, dd_first_line, "line1\nline2\nline3", "dd");
neovim_test!(operators, dd_last_line, "line1\nline2\nline3", cursor(2, 0), "dd");
neovim_test!(operators, dd_only_line, "only", "dd");
neovim_test!(operators, dd_with_count, "l1\nl2\nl3\nl4", "2dd");

// d with count
neovim_test!(operators, d2w_two_words, "one two three", "d2w");
neovim_test!(operators, d3l_three_right, "hello world", "d3l");

// d with find
neovim_test!(operators, df_find, "hello world", "dfo");
neovim_test!(operators, dt_till, "hello world", "dto");

// NEW: d edge cases
neovim_test!(operators, dw_empty_buffer, "", "dw");
neovim_test!(operators, dd_empty_buffer, "", "dd");
neovim_test!(operators, dw_at_eof, "hello", cursor(0, 4), "dw");
neovim_test!(operators, dd_last_line_no_newline, "line1\nline2", cursor(1, 0), "dd");
neovim_test!(operators, dw_single_char, "a", "dw");
neovim_test!(operators, dw_at_eol, "hello world", cursor(0, 5), "dw");
neovim_test!(operators, dd_count_past_eof, "l1\nl2\nl3", "100dd");
neovim_test!(operators, dj_two_lines, "line1\nline2\nline3", "dj");

// Vi blank-line delete promotion: charwise multiline delete with blank
// remaining text promotes to linewise (Neovim ops.c:742-757)
neovim_test!(operators, d_close_brace_blank_promotion, "hello\n\nworld", "d}");
neovim_test!(operators, dk_two_lines, "line1\nline2\nline3", cursor(1, 0), "dk");
neovim_test!(operators, d_caret_to_first_nonblank, "  hello world", cursor(0, 7), "d^");
neovim_test!(operators, dG_to_eof, "l1\nl2\nl3\nl4", "dG");
neovim_test!(operators, dgg_to_bof, "l1\nl2\nl3\nl4", cursor(2, 0), "dgg");
neovim_test!(operators, df_not_found, "hello", "dfz");
neovim_test!(operators, dt_not_found, "hello", "dtz");
neovim_test!(operators, d_percent_bracket, "(hello)", "d%");
neovim_test!(operators, D_to_eol, "hello world", cursor(0, 5), "D");
neovim_test!(operators, D_at_bol, "hello world", "D");
neovim_test!(operators, D_at_eol, "hello", cursor(0, 4), "D");
neovim_test!(operators, d_unicode_word, "日本語 hello", "dw");
neovim_test!(operators, dd_unicode_line, "日本語", "dd");
neovim_test!(operators, daw_around_word, "hello world", cursor(0, 0), "daw");
neovim_test!(operators, diw_inner_word, "hello world", cursor(0, 0), "diw");
neovim_test!(operators, dap_around_paragraph, "line1\nline2\n\nline3", "dap");
neovim_test!(operators, dip_inner_paragraph, "line1\nline2\n\nline3", "dip");

// ═══════════════════════════════════════════════════════════════════════════════
// YANK OPERATOR (y)
// ═══════════════════════════════════════════════════════════════════════════════

// y{motion} - yank with motion
neovim_test!(operators, yw_word, "hello world", "yw");
neovim_test!(operators, ye_word_end, "hello world", "ye");
neovim_test!(operators, y_dollar_to_eol, "hello world", "y$");
neovim_test!(operators, y_zero_to_bol, "hello world", cursor(0, 5), "y0");

// yy - yank line (cursor stays at start)
neovim_test!(operators, yy_single, "line1\nline2\nline3", cursor(1, 0), "yy");
neovim_test!(operators, yy_first_line, "line1\nline2\nline3", "yy");
neovim_test!(operators, yy_with_count, "l1\nl2\nl3\nl4", "2yy");

// p - paste after yank (validates yank worked)
neovim_test!(operators, yw_p_paste, "hello world", "ywp");
neovim_test!(operators, yy_p_paste_line, "line1\nline2", "yyp");
neovim_test!(operators, dd_p_paste_deleted, "line1\nline2\nline3", cursor(1, 0), "ddp");

// NEW: y edge cases
neovim_test!(operators, yw_empty_buffer, "", "yw");
neovim_test!(operators, yy_empty_buffer, "", "yy");
neovim_test!(operators, yw_single_char, "a", "yw");
neovim_test!(operators, yy_single_line, "hello", "yy");
neovim_test!(operators, yy_count_past_eof, "l1\nl2\nl3", "100yy");
neovim_test!(operators, yj_two_lines, "line1\nline2\nline3", "yj");
neovim_test!(operators, yk_two_lines, "line1\nline2\nline3", cursor(1, 0), "yk");
neovim_test!(operators, y_caret, "  hello world", cursor(0, 7), "y^");
neovim_test!(operators, yG_to_eof, "l1\nl2\nl3\nl4", "yG");
neovim_test!(operators, ygg_to_bof, "l1\nl2\nl3\nl4", cursor(2, 0), "ygg");
neovim_test!(operators, yf_find, "hello world", "yfo");
neovim_test!(operators, yt_till, "hello world", "yto");
neovim_test!(operators, y_percent_bracket, "(hello)", "y%");
neovim_test!(operators, Y_line, "hello world\nline2", "Y");
neovim_test!(operators, y_unicode_word, "日本語 hello", "yw");
neovim_test!(operators, yy_unicode_line, "日本語", "yy");
neovim_test!(operators, yaw_around_word, "hello world", cursor(0, 0), "yaw");
neovim_test!(operators, yiw_inner_word, "hello world", cursor(0, 0), "yiw");
neovim_test!(operators, yy_p_below, "line1\nline2", "yyjp");
neovim_test!(operators, yy_P_above, "line1\nline2", cursor(1, 0), "yyP");

// ═══════════════════════════════════════════════════════════════════════════════
// CHANGE OPERATOR (c)
// ═══════════════════════════════════════════════════════════════════════════════

// c{motion} - change with motion
neovim_test!(operators, cw_word, "hello world", "cwbye<Esc>");
neovim_test!(operators, ce_word_end, "hello world", "cebye<Esc>");
neovim_test!(operators, c_dollar_to_eol, "hello world", "c$end<Esc>");
neovim_test!(operators, ch_left, "hello", cursor(0, 2), "chX<Esc>");
neovim_test!(operators, cl_right, "hello", "clX<Esc>");

// cc - change line
neovim_test!(operators, cc_single, "line1\nline2\nline3", cursor(1, 0), "ccnew<Esc>");
neovim_test!(operators, cc_first_line, "line1\nline2", "ccfirst<Esc>");
neovim_test!(operators, cc_with_count, "l1\nl2\nl3\nl4", "2ccnew<Esc>");

// C - change to end of line (same as c$)
neovim_test!(operators, C_to_eol, "hello world", cursor(0, 5), "Cend<Esc>");

// cw vs ce edge case (at word boundary)
neovim_test!(operators, cw_at_word_end, "hello world", cursor(0, 4), "cwbye<Esc>");

// NEW: c edge cases
neovim_test!(operators, cw_empty_buffer, "", "cwtest<Esc>");
neovim_test!(operators, cc_empty_buffer, "", "cctest<Esc>");
neovim_test!(operators, cw_single_char, "a", "cwtest<Esc>");
neovim_test!(operators, cc_single_line, "hello", "ccnew<Esc>");
neovim_test!(operators, cc_count_past_eof, "l1\nl2\nl3", "100ccnew<Esc>");
neovim_test!(operators, cj_two_lines, "line1\nline2\nline3", "cjnew<Esc>");
neovim_test!(operators, ck_two_lines, "line1\nline2\nline3", cursor(1, 0), "cknew<Esc>");
neovim_test!(operators, c_caret, "  hello world", cursor(0, 7), "c^X<Esc>");
neovim_test!(operators, cG_to_eof, "l1\nl2\nl3\nl4", "cGnew<Esc>");
neovim_test!(operators, cgg_to_bof, "l1\nl2\nl3\nl4", cursor(2, 0), "cggnew<Esc>");
neovim_test!(operators, cf_find, "hello world", "cfoX<Esc>");
neovim_test!(operators, ct_till, "hello world", "ctoX<Esc>");
neovim_test!(operators, c_percent_bracket, "(hello)", "c%X<Esc>");
neovim_test!(operators, C_at_bol, "hello world", "Cend<Esc>");
neovim_test!(operators, C_at_eol, "hello", cursor(0, 4), "Cx<Esc>");
neovim_test!(operators, c_unicode_word, "日本語 hello", "cwtest<Esc>");
neovim_test!(operators, cc_unicode_line, "日本語", "cctest<Esc>");
neovim_test!(operators, caw_around_word, "hello world", cursor(0, 0), "cawX<Esc>");
neovim_test!(operators, ciw_inner_word, "hello world", cursor(0, 0), "ciwX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// INDENT OPERATORS (>, <)
// ═══════════════════════════════════════════════════════════════════════════════

// >> - indent line
neovim_test!(operators, indent_line, "hello", ">>");
neovim_test!(operators, indent_multiline, "l1\nl2\nl3", "2>>");
neovim_test!(operators, indent_already_indented, "  hello", ">>");

// << - outdent line
neovim_test!(operators, outdent_line, "    hello", "<<");
neovim_test!(operators, outdent_multiline, "    l1\n    l2\n    l3", "2<<");
neovim_test!(operators, outdent_no_indent, "hello", "<<");
neovim_test!(operators, outdent_partial, "  hello", "<<");

// >j, <k - indent with motion
neovim_test!(operators, indent_motion_j, "l1\nl2\nl3", ">j");

// NEW: indent edge cases
neovim_test!(operators, indent_empty_buffer, "", ">>");
neovim_test!(operators, outdent_empty_buffer, "", "<<");
neovim_test!(operators, indent_count_past_eof, "l1\nl2", "100>>");
neovim_test!(operators, outdent_count_past_eof, "    l1\n    l2", "100<<");
neovim_test!(operators, indent_motion_G, "l1\nl2\nl3", ">G");
neovim_test!(operators, outdent_motion_G, "    l1\n    l2\n    l3", "<G");
neovim_test!(operators, indent_motion_gg, "l1\nl2\nl3", cursor(2, 0), ">gg");
neovim_test!(operators, indent_tabs, "\thello", ">>");
neovim_test!(operators, outdent_tabs, "\t\thello", "<<");
neovim_test!(operators, indent_unicode, "日本語", ">>");
neovim_test!(operators, indent_paragraph, "l1\nl2\n\nl3", ">}");
neovim_test!(operators, indent_visual_line, "l1\nl2\nl3", "Vj>");

// ═══════════════════════════════════════════════════════════════════════════════
// CASE OPERATORS (gu, gU, g~)
// ═══════════════════════════════════════════════════════════════════════════════

// gu - lowercase
neovim_test!(operators, gu_word, "HELLO world", "guw");
neovim_test!(operators, guu_line, "HELLO WORLD", "guu");
neovim_test!(operators, gu_motion, "HELLO", cursor(0, 0), "gu$");

// gU - uppercase
neovim_test!(operators, gU_word, "hello WORLD", "gUw");
neovim_test!(operators, gUU_line, "hello world", "gUU");
neovim_test!(operators, gU_motion, "hello", cursor(0, 0), "gU$");

// g~ - toggle case
neovim_test!(operators, g_tilde_word, "HeLLo wORLD", "g~w");
neovim_test!(operators, g_tilde_line, "HeLLo", "g~~");
neovim_test!(operators, g_tilde_motion, "HeLLo", cursor(0, 0), "g~$");

// ~ - toggle case single char (visual mode would be different)
neovim_test!(operators, tilde_char, "hello", "~");

// NEW: case edge cases
neovim_test!(operators, gu_empty_buffer, "", "guw");
neovim_test!(operators, gU_empty_buffer, "", "gUw");
neovim_test!(operators, g_tilde_empty_buffer, "", "g~w");
neovim_test!(operators, gu_single_char, "A", "guw");
neovim_test!(operators, gU_single_char, "a", "gUw");
neovim_test!(operators, g_tilde_single_char, "a", "g~w");
neovim_test!(operators, guu_unicode, "HÉLLO WÖRLD", "guu");
neovim_test!(operators, gUU_unicode, "héllo wörld", "gUU");
neovim_test!(operators, g_tilde_unicode, "HéLLo", "g~~");
neovim_test!(operators, gu_motion_iw, "HELLO world", "guiw");
neovim_test!(operators, gU_motion_iw, "hello WORLD", "gUiw");
neovim_test!(operators, gu_visual, "HELLO world", "vwgu");
neovim_test!(operators, gU_visual, "hello world", "vwgU");
neovim_test!(operators, tilde_with_count, "hello", "3~");
neovim_test!(operators, tilde_at_eol, "hello", cursor(0, 4), "~");
neovim_test!(operators, guG_to_eof, "HELLO\nWORLD", "guG");
neovim_test!(operators, gUG_to_eof, "hello\nworld", "gUG");

// ═══════════════════════════════════════════════════════════════════════════════
// FORMAT OPERATOR (gq)
// ═══════════════════════════════════════════════════════════════════════════════

// gq{motion} - format
neovim_test!(operators, gq_line, "hello world this is a very long line that might wrap", "gqq");
neovim_test!(operators, gq_paragraph, "line1\nline2\n\nline3", "gq}");
neovim_test!(operators, gq_motion_j, "l1\nl2", "gqj");

// gqq - format current line
neovim_test!(operators, gqq_basic, "hello world", "gqq");
neovim_test!(operators, gqq_empty, "", "gqq");

// NEW: gq edge cases
neovim_test!(operators, gq_single_word, "hello", "gqq");
neovim_test!(operators, gqG_to_eof, "l1\nl2\nl3", "gqG");
neovim_test!(operators, gq_visual, "hello world", "Vgq");
neovim_test!(operators, gq_unicode, "日本語", "gqq");

// ═══════════════════════════════════════════════════════════════════════════════
// COMBINED OPERATOR + MOTION
// ═══════════════════════════════════════════════════════════════════════════════

// Operators with various motions
neovim_test!(operators, d_gg_to_start, "l1\nl2\nl3\nl4", cursor(2, 0), "dgg");
neovim_test!(operators, d_G_to_end, "l1\nl2\nl3\nl4", "dG");
neovim_test!(operators, y_gg_to_start, "l1\nl2\nl3\nl4", cursor(2, 0), "ygg");
neovim_test!(operators, c_gg_to_start, "l1\nl2\nl3\nl4", cursor(2, 0), "cggnew<Esc>");

// Operators with word motions
neovim_test!(operators, dW_WORD, "foo.bar baz", "dW");
neovim_test!(operators, dE_WORD_end, "foo.bar baz", "dE");
neovim_test!(operators, dB_WORD_back, "foo.bar baz", cursor(0, 8), "dB");

// Operators with paragraph motions
neovim_test!(operators, d_brace_paragraph, "line1\nline2\n\nline3\nline4", "d}");
neovim_test!(operators, y_brace_paragraph, "line1\nline2\n\nline3\nline4", "y}");

// NEW: More combined operator tests
neovim_test!(operators, d_percent, "if (x) { y }", "d%");
neovim_test!(operators, c_percent, "if (x) { y }", "c%changed<Esc>");
neovim_test!(operators, y_percent, "if (x) { y }", "y%");
neovim_test!(operators, d_f_semicolon_repeat, "a;b;c;d", "df;.");
neovim_test!(operators, c_f_find, "hello world", "cfoX<Esc>");
neovim_test!(operators, d_t_semicolon_repeat, "a;b;c;d", "dt;.");
neovim_test!(operators, d_slash_search, "hello world test", "d/test<CR>");
neovim_test!(operators, c_slash_search, "hello world test", "c/test<CR>X<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// REGISTER OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

// Named register operations
neovim_test!(operators, register_a_yank, "hello world", "\"ayw");
neovim_test!(operators, register_a_delete, "hello world", "\"adw");
neovim_test!(operators, register_a_paste, "hello world", "\"ayw$\"ap");

// NEW: Register edge cases
neovim_test!(operators, register_0_yank, "hello world", "yw\"0p");
neovim_test!(operators, register_blackhole_delete, "hello world", "\"_dw");
neovim_test!(operators, register_append_A, "hello world", "\"ayw\"Ayw$\"ap");
neovim_test!(operators, register_z_yank, "hello world", "\"zyw");
neovim_test!(operators, register_unnamed_after_delete, "hello world", "dwp");
neovim_test!(operators, register_unnamed_after_yank, "hello world", "ywp");
neovim_test!(operators, register_dd_numbered, "l1\nl2\nl3", "dd\"1p");
neovim_test!(operators, register_yy_named, "hello", "\"ayy\"ap");
neovim_test!(operators, register_delete_line_paste, "l1\nl2\nl3", "\"add\"ap");
neovim_test!(operators, register_yank_visual, "hello world", "vw\"ay$\"ap");

// ═══════════════════════════════════════════════════════════════════════════════
// TEXT OBJECT OPERATIONS WITH OPERATORS
// ═══════════════════════════════════════════════════════════════════════════════

// i/a word
neovim_test!(operators, diw_in_word, "hello world", cursor(0, 2), "diw");
neovim_test!(operators, daw_in_word, "hello world", cursor(0, 2), "daw");
neovim_test!(operators, ciw_in_word, "hello world", cursor(0, 2), "ciwX<Esc>");
neovim_test!(operators, yiw_in_word, "hello world", cursor(0, 2), "yiw$p");

// i/a quotes
neovim_test!(operators, di_dquote, "hello \"world\" test", cursor(0, 8), "di\"");
neovim_test!(operators, da_dquote, "hello \"world\" test", cursor(0, 8), "da\"");
neovim_test!(operators, ci_dquote, "hello \"world\" test", cursor(0, 8), "ci\"X<Esc>");
neovim_test!(operators, yi_dquote, "hello \"world\" test", cursor(0, 8), "yi\"$p");

// i/a parens
neovim_test!(operators, di_paren, "hello (world) test", cursor(0, 8), "di(");
neovim_test!(operators, da_paren, "hello (world) test", cursor(0, 8), "da(");
neovim_test!(operators, ci_paren, "hello (world) test", cursor(0, 8), "ci(X<Esc>");
neovim_test!(operators, yi_paren, "hello (world) test", cursor(0, 8), "yi($p");

// i/a braces
neovim_test!(operators, di_brace, "hello {world} test", cursor(0, 8), "di{");
neovim_test!(operators, da_brace, "hello {world} test", cursor(0, 8), "da{");

// i/a brackets
neovim_test!(operators, di_bracket, "hello [world] test", cursor(0, 8), "di[");
neovim_test!(operators, da_bracket, "hello [world] test", cursor(0, 8), "da[");

// i/a sentence
neovim_test!(operators, dis_sentence, "Hello world. Goodbye world.", cursor(0, 5), "dis");
neovim_test!(operators, das_sentence, "Hello world. Goodbye world.", cursor(0, 5), "das");

// i/a paragraph
neovim_test!(operators, dip_paragraph, "line1\nline2\n\nline3", cursor(0, 0), "dip");
neovim_test!(operators, dap_paragraph_full, "line1\nline2\n\nline3", cursor(0, 0), "dap");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES - COMPREHENSIVE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(operators, cw_unicode, "héllo wörld", "cwbye<Esc>");
neovim_test!(operators, dw_tabs_and_spaces, "\t  hello", "dw");

// Operator at various boundaries
neovim_test!(operators, dw_at_bol, "hello world", "dw");
neovim_test!(operators, dw_at_middle, "hello world", cursor(0, 6), "dw");
neovim_test!(operators, dd_with_indent, "  hello", "dd");
neovim_test!(operators, cc_with_indent, "  hello", "ccX<Esc>");
neovim_test!(operators, yy_with_indent, "  hello", "yyp");

// Multiple operations
neovim_test!(operators, dd_dd_two_lines, "l1\nl2\nl3", "dddd");
neovim_test!(operators, dw_dw_two_words, "one two three", "dwdw");
neovim_test!(operators, yy_yy_paste, "l1\nl2", "yyjyyp");

// Operators with empty/single char
neovim_test!(operators, dl_single_char, "a", "dl");
neovim_test!(operators, cl_single_char, "a", "clX<Esc>");
neovim_test!(operators, yl_single_char, "a", "ylp");

// Visual mode operators
neovim_test!(operators, v_d_delete, "hello world", "vwd");
neovim_test!(operators, v_y_yank, "hello world", "vwyp");
neovim_test!(operators, v_c_change, "hello world", "vwcX<Esc>");
neovim_test!(operators, V_d_line, "l1\nl2\nl3", "Vd");
neovim_test!(operators, V_y_line, "l1\nl2\nl3", "Vyp");
neovim_test!(operators, V_c_line, "l1\nl2\nl3", "VcX<Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Operator + Boundary Edge Cases
// ─────────────────────────────────────────────────────────────────────────────

// dG at last line (delete last line)
neovim_test!(operators, dG_at_last_line, "aaa\nbbb\nccc", cursor(2, 0), "dG");
// dgg at first line (delete first line)
neovim_test!(operators, dgg_at_first_line, "aaa\nbbb\nccc", "dgg");
// D at EOL (should be no-op or delete nothing)
neovim_test!(operators, D_at_eol_boundary, "hello", cursor(0, 4), "D");
// dd on only line
neovim_test!(operators, dd_only_line_boundary, "hello", "dd");
// yy on empty line
neovim_test!(operators, yy_empty_line, "\n\n\n", cursor(1, 0), "yyp");
// cc on empty line
neovim_test!(operators, cc_empty_line, "\n\n\n", cursor(1, 0), "ccnew<Esc>");
// dj at second-to-last line
neovim_test!(operators, dj_near_end, "aaa\nbbb\nccc", cursor(1, 0), "dj");
// dk at second line
neovim_test!(operators, dk_near_start, "aaa\nbbb\nccc", cursor(1, 0), "dk");

// ─────────────────────────────────────────────────────────────────────────────
// Linewise paste edge cases
// ─────────────────────────────────────────────────────────────────────────────

// Paste linewise register at middle of doc
neovim_test!(operators, paste_linewise_mid, "aaa\nbbb\nccc", "yyjp");
// P (paste before) linewise
neovim_test!(operators, paste_linewise_before, "aaa\nbbb\nccc", cursor(2, 0), "yyP");
// Paste charwise on empty line
neovim_test!(operators, paste_charwise_empty_line, "hello\n\nworld", "ywjp");
// dd then p at last line
neovim_test!(operators, dd_p_at_last, "aaa\nbbb\nccc", cursor(2, 0), "ddp");
// Multiple dd then numbered register
neovim_test!(operators, dd_numbered_regs, "aaa\nbbb\nccc\nddd", "dddddd\"1p");

// ─────────────────────────────────────────────────────────────────────────────
// Operator with search motion
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(operators, d_search_fwd, "hello world test foo", "d/test<CR>");
neovim_test!(operators, c_search_fwd, "hello world test foo", "c/test<CR>X<Esc>");
neovim_test!(operators, y_search_fwd, "hello world test foo", "y/test<CR>$p");
neovim_test!(operators, d_search_bwd, "hello world test foo", cursor(0, 17), "d?test<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATORS WITH SEARCH OBJECT (ygn)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(operators, ygn_basic, "hello world hello", "/hello<CR>ygnp");

// ═══════════════════════════════════════════════════════════════════════════════
// EXCLUSIVE-TO-LINEWISE PROMOTION (:h exclusive-linewise)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Neovim rule: When an exclusive motion's end is at column 0:
//   - If start is at or before the first non-blank → promote to linewise
//   - Otherwise → back up end to end of previous line (inclusive, not linewise)
//
// These tests verify the exact Neovim behavior for the exclusive-linewise rule.

// --- Promotion to linewise: start at/before first non-blank ---

// d/pattern where search lands at col 0 of next line, cursor at col 0 (= first non-blank)
// → should promote to linewise (delete entire line including newline)
neovim_test!(operators, excl_lw_d_search_col0_promote, "hello\nworld\nafter", "d/world<CR>");

// d/pattern with indented start, cursor at col 0 (before first non-blank)
// → should promote to linewise
neovim_test!(operators, excl_lw_d_search_indented_promote, "  hello\nworld\nafter", "d/world<CR>");

// yank version: y/pattern at col 0, verify yanked content
neovim_test!(operators, excl_lw_y_search_col0_promote, "hello\nworld\nafter", "y/world<CR>p");

// --- No promotion: start after first non-blank ---

// d/pattern where cursor is after first non-blank → should NOT promote to linewise
// Just backs up end to end of previous line (inclusive)
neovim_test!(operators, excl_lw_d_search_no_promote, "hello\nworld\nafter", cursor(0, 2), "d/world<CR>");

// d/pattern with indented line, cursor on a non-blank char after first non-blank
neovim_test!(operators, excl_lw_d_search_indented_no_promote, "  hello\nworld\nafter", cursor(0, 4), "d/world<CR>");

// --- Edge case: d} paragraph motion (paragraph motions have special handling) ---
neovim_test!(operators, excl_lw_d_brace_basic, "hello\nworld\n\nafter", "d}");

// d} from indented start
neovim_test!(operators, excl_lw_d_brace_indented, "  hello\n  world\n\nafter", "d}");

// --- Multi-line exclusive motions ending at col 0 ---

// d/pattern spanning multiple lines with start at col 0
neovim_test!(operators, excl_lw_d_search_multiline_promote, "line1\nline2\nline3\ntarget", "d/target<CR>");

// Same but cursor in middle of line → no promotion
neovim_test!(operators, excl_lw_d_search_multiline_no_promote, "line1\nline2\nline3\ntarget", cursor(0, 2), "d/target<CR>");

// --- Change operator versions (verify c uses same rule) ---
neovim_test!(operators, excl_lw_c_search_col0_promote, "hello\nworld\nafter", "c/world<CR>X<Esc>");
neovim_test!(operators, excl_lw_c_search_no_promote, "hello\nworld\nafter", cursor(0, 2), "c/world<CR>X<Esc>");

// --- Yank operator: verify register type is linewise when promoted ---
neovim_test!(operators, excl_lw_y_search_promote_paste, "hello\nworld\nafter", "y/world<CR>Gp");
neovim_test!(operators, excl_lw_y_search_no_promote_paste, "hello\nworld\nafter", cursor(0, 2), "y/world<CR>$p");

// --- Empty previous line: edge case where end is at col 0 but previous line is empty ---
neovim_test!(operators, excl_lw_d_search_empty_prev_line, "hello\n\nworld", cursor(0, 2), "d/world<CR>");

// --- dw across lines (w lands at col 0 of next line) ---
neovim_test!(operators, excl_lw_dw_eol_wraps, "hello\nworld", cursor(0, 4), "dw");
neovim_test!(operators, excl_lw_dw_eol_wraps_indented, "  hello\nworld", cursor(0, 6), "dw");

// ═══════════════════════════════════════════════════════════════════════════════
// ORACLE REGRESSION FIXES — Cursor positioning after operators
// ═══════════════════════════════════════════════════════════════════════════════

// Bug 1: gukFp — linewise gu with k motion should preserve cursor column
// (Neovim sets cursor to oap->start = min(cursor, motion_target))
neovim_test!(operators, guk_cursor_col_preserved,
    "        mnynsriyhn\n                gmjh\nxcwsk\n        cazamlwijh\n                potrmdrlo\naatz\n    cszcguodow",
    cursor(5, 3), "gukFp");

// Bug 2: yi{ — cursor_col should be byte-based (not grapheme-based)
// Tests that cursor_col matches Neovim's byte column for multi-byte text
neovim_test!(operators, yi_brace_multibyte_cursor_col,
    "\u{1f30d} hello \u{4e16}\u{754c}", cursor(0, 8), "yi{");

// Bug 5: >hgU^^yG — cursor after indent + case + yank sequence
// After indent, gU operates with preserved column, yG preserves cursor
neovim_test!(operators, indent_case_yank_cursor,
    "qwykcbbi nbflj eqdulzd qucjl clcqu lwoa bfmgc rqa",
    cursor(0, 20), ">hgU^^yG");

// Linewise yank preserves cursor position for forward motions (yG)
neovim_test!(operators, yG_cursor_stays_forward,
    "    hello world", cursor(0, 4), "yG");

// Linewise yank moves cursor to motion target for backward motions (y'a)
neovim_test!(operators, yank_to_mark_backward,
    "hello world", "may'a");

// Linewise case operator preserves column across lines (guk)
neovim_test!(operators, gu_k_linewise_cursor,
    "HELLO\nWORLD", cursor(1, 3), "guk");

// gUU on line — cursor stays at min of first-non-blank and original position
neovim_test!(operators, gUU_cursor_at_start,
    "  hello world", "gUU");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATOR + PARAGRAPH / v-FORCE (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// d$ deletes to end of line (charwise)
neovim_test!(operators, nf_d_dollar, "aaa\nbbb\nccc", "d$");

// d} linewise paragraph delete
neovim_test!(operators, nf_d_paragraph, "aaa\nbbb\nccc\n", "d}");

// dv} charwise-forced paragraph delete (differs from d})
neovim_test!(operators, nf_dv_paragraph, "aaa\nbbb\nccc\n", "dv}");

// Bug hunt: y2j + Gp at end of file
neovim_test!(operators, yank_2j_paste_at_end, "one\ntwo\nthree\nfour\n", "y2jGp");

// Bug hunt: dG from line 1
neovim_test!(operators, dG_from_line_1, "first\nsecond\nthird\nfourth\nfifth\n", cursor(1, 0), "dG");
