// Operator × Motion Matrix fidelity tests.
//
// Systematic coverage: every operator with every motion class.
// Operators: d, c, y, >, <, gu, gU, g~, gq, =
// Motions: h, l, w, e, b, W, E, B, ge, gE, 0, $, ^, g_, f, t, F, T,
//          j, k, +, -, gg, G, {, }, (, ), %, H, M, L, /search, ?search,
//          iw, aw, i(, a(, i{, a{, i[, a[, i", a", is, as, ip, ap

// ═══════════════════════════════════════════════════════════════════════════════
// DELETE (d) × ALL MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

// d + character motions
neovim_test!(scenarios, d_h, "hello world", cursor(0, 5), "dh");
neovim_test!(scenarios, d_l, "hello world", "dl");
neovim_test!(scenarios, d_2h, "hello world", cursor(0, 5), "d2h");
neovim_test!(scenarios, d_3l, "hello world", "d3l");

// d + word motions
neovim_test!(scenarios, d_w, "hello world test", "dw");
neovim_test!(scenarios, d_e, "hello world test", "de");
neovim_test!(scenarios, d_b, "hello world test", cursor(0, 6), "db");
neovim_test!(scenarios, d_W, "foo.bar baz.qux", "dW");
neovim_test!(scenarios, d_E, "foo.bar baz.qux", "dE");
neovim_test!(scenarios, d_B, "foo.bar baz.qux", cursor(0, 8), "dB");
neovim_test!(scenarios, matrix_d_ge, "hello world test", cursor(0, 11), "dge");
neovim_test!(scenarios, matrix_d_gE, "hello-world test.foo", cursor(0, 16), "dgE");

// d + line position motions
neovim_test!(scenarios, d_0, "hello world", cursor(0, 5), "d0");
neovim_test!(scenarios, d_dollar, "hello world", "d$");
neovim_test!(scenarios, d_caret, "  hello world", cursor(0, 7), "d^");
neovim_test!(scenarios, d_g_underscore_op, "hello world   ", "dg_");

// d + find/till motions
neovim_test!(scenarios, d_f_o, "hello world", "dfo");
neovim_test!(scenarios, d_t_o, "hello world", "dto");
neovim_test!(scenarios, d_F_o, "hello world", cursor(0, 10), "dFo");
neovim_test!(scenarios, d_T_o, "hello world", cursor(0, 10), "dTo");
neovim_test!(scenarios, d_2f_o, "fooboo moo", "d2fo");
neovim_test!(scenarios, d_2t_o, "fooboo moo", "d2to");

// d + line motions
neovim_test!(scenarios, d_j, "line1\nline2\nline3", "dj");
neovim_test!(scenarios, d_k, "line1\nline2\nline3", cursor(1, 0), "dk");
neovim_test!(scenarios, d_2j, "l1\nl2\nl3\nl4", "d2j");
neovim_test!(scenarios, d_2k, "l1\nl2\nl3\nl4", cursor(3, 0), "d2k");
neovim_test!(scenarios, matrix_d_plus, "line1\n  line2\nline3", "d+");
neovim_test!(scenarios, matrix_d_minus, "line1\n  line2\nline3", cursor(2, 0), "d-");

// d + document motions
neovim_test!(scenarios, d_gg, "l1\nl2\nl3\nl4", cursor(2, 0), "dgg");
neovim_test!(scenarios, d_G, "l1\nl2\nl3\nl4", "dG");
neovim_test!(scenarios, d_2G, "l1\nl2\nl3\nl4", "d2G");
neovim_test!(scenarios, d_2gg, "l1\nl2\nl3\nl4", cursor(3, 0), "d2gg");

// d + paragraph/sentence
neovim_test!(scenarios, d_open_brace, "para1\n\npara2\n\npara3", cursor(2, 0), "d{");
neovim_test!(scenarios, d_close_brace, "para1\n\npara2\n\npara3", "d}");
neovim_test!(scenarios, d_open_paren, "Hello. World. End.", cursor(0, 7), "d(");
neovim_test!(scenarios, d_close_paren, "Hello. World. End.", "d)");

// d + percent
neovim_test!(scenarios, d_percent_paren, "(hello world)", "d%");
neovim_test!(scenarios, d_percent_brace, "{hello world}", "d%");
neovim_test!(scenarios, matrix_d_percent_bracket, "[hello world]", "d%");
neovim_test!(scenarios, d_percent_from_close, "(hello world)", cursor(0, 12), "d%");

// d + screen motions
neovim_test!(scenarios, matrix_d_H, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(5, 0), "dH");
neovim_test!(scenarios, matrix_d_M, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "dM");
neovim_test!(scenarios, matrix_d_L, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "dL");

// d + search
neovim_test!(scenarios, d_search_fwd_matrix, "hello world test end", "d/test<CR>");
neovim_test!(scenarios, d_search_bwd_matrix, "hello world test end", cursor(0, 17), "d?test<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// CHANGE (c) × ALL MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

// c + character motions
neovim_test!(scenarios, c_h, "hello world", cursor(0, 5), "chX<Esc>");
neovim_test!(scenarios, c_l, "hello world", "clX<Esc>");
neovim_test!(scenarios, c_2h, "hello world", cursor(0, 5), "c2hX<Esc>");
neovim_test!(scenarios, c_3l, "hello world", "c3lX<Esc>");

// c + word motions
neovim_test!(scenarios, c_w, "hello world test", "cwX<Esc>");
neovim_test!(scenarios, c_e, "hello world test", "ceX<Esc>");
neovim_test!(scenarios, c_b, "hello world test", cursor(0, 6), "cbX<Esc>");
neovim_test!(scenarios, c_W, "foo.bar baz.qux", "cWX<Esc>");
neovim_test!(scenarios, c_E, "foo.bar baz.qux", "cEX<Esc>");
neovim_test!(scenarios, c_B, "foo.bar baz.qux", cursor(0, 8), "cBX<Esc>");
neovim_test!(scenarios, matrix_c_ge, "hello world test", cursor(0, 11), "cgeX<Esc>");
neovim_test!(scenarios, c_gE, "hello-world test.foo", cursor(0, 16), "cgEX<Esc>");

// c + line position motions
neovim_test!(scenarios, c_0, "hello world", cursor(0, 5), "c0X<Esc>");
neovim_test!(scenarios, c_dollar, "hello world", "c$X<Esc>");
neovim_test!(scenarios, matrix_c_caret, "  hello world", cursor(0, 7), "c^X<Esc>");
neovim_test!(scenarios, c_g_underscore_op, "hello world   ", "cg_X<Esc>");

// c + find/till motions
neovim_test!(scenarios, c_f_o, "hello world", "cfoX<Esc>");
neovim_test!(scenarios, c_t_o, "hello world", "ctoX<Esc>");
neovim_test!(scenarios, c_F_o, "hello world", cursor(0, 10), "cFoX<Esc>");
neovim_test!(scenarios, c_T_o, "hello world", cursor(0, 10), "cToX<Esc>");

// c + line motions
neovim_test!(scenarios, c_j, "line1\nline2\nline3", "cjX<Esc>");
neovim_test!(scenarios, c_k, "line1\nline2\nline3", cursor(1, 0), "ckX<Esc>");
neovim_test!(scenarios, matrix_c_plus, "line1\n  line2\nline3", "c+X<Esc>");
neovim_test!(scenarios, c_minus, "line1\n  line2\nline3", cursor(2, 0), "c-X<Esc>");

// c + document motions
neovim_test!(scenarios, c_gg, "l1\nl2\nl3\nl4", cursor(2, 0), "cggX<Esc>");
neovim_test!(scenarios, c_G, "l1\nl2\nl3\nl4", "cGX<Esc>");

// c + paragraph/sentence
neovim_test!(scenarios, c_open_brace, "para1\n\npara2", cursor(2, 0), "c{X<Esc>");
neovim_test!(scenarios, c_close_brace, "para1\n\npara2", "c}X<Esc>");
neovim_test!(scenarios, c_close_paren, "Hello. World.", "c)X<Esc>");

// c + percent
neovim_test!(scenarios, c_percent_paren, "(hello world)", "c%X<Esc>");
neovim_test!(scenarios, c_percent_brace, "{hello world}", "c%X<Esc>");

// c + search
neovim_test!(scenarios, c_search_fwd_matrix, "hello world test end", "c/test<CR>X<Esc>");
neovim_test!(scenarios, c_search_bwd_matrix, "hello world test end", cursor(0, 17), "c?test<CR>X<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// YANK (y) × ALL MOTIONS (verify with paste)
// ═══════════════════════════════════════════════════════════════════════════════

// y + character motions
neovim_test!(scenarios, y_h, "hello world", cursor(0, 5), "yhp");
neovim_test!(scenarios, y_l, "hello world", "ylp");
neovim_test!(scenarios, y_2l, "hello world", "y2lp");
neovim_test!(scenarios, y_3h, "hello world", cursor(0, 5), "y3hp");

// y + word motions
neovim_test!(scenarios, y_w, "hello world test", "yw$p");
neovim_test!(scenarios, y_e, "hello world test", "ye$p");
neovim_test!(scenarios, y_b, "hello world test", cursor(0, 6), "yb0P");
neovim_test!(scenarios, y_W, "foo.bar baz.qux", "yW$p");
neovim_test!(scenarios, y_E, "foo.bar baz.qux", "yE$p");
neovim_test!(scenarios, y_B, "foo.bar baz.qux", cursor(0, 8), "yB0P");
neovim_test!(scenarios, y_ge, "hello world test", cursor(0, 11), "yge0P");
neovim_test!(scenarios, y_gE, "hello-world test.foo", cursor(0, 16), "ygE0P");

// y + line position motions
neovim_test!(scenarios, y_0, "hello world", cursor(0, 5), "y0P");
neovim_test!(scenarios, y_dollar, "hello world", "y$p");
neovim_test!(scenarios, matrix_y_caret, "  hello world", cursor(0, 7), "y^0P");
neovim_test!(scenarios, y_g_underscore_op, "hello world   ", "yg_$p");

// y + find/till motions
neovim_test!(scenarios, y_f_o, "hello world", "yfo$p");
neovim_test!(scenarios, y_t_o, "hello world", "yto$p");
neovim_test!(scenarios, y_F_o, "hello world", cursor(0, 10), "yFo0P");
neovim_test!(scenarios, y_T_o, "hello world", cursor(0, 10), "yTo0P");

// y + line motions (linewise yank)
neovim_test!(scenarios, y_j, "line1\nline2\nline3", "yjGp");
neovim_test!(scenarios, y_k, "line1\nline2\nline3", cursor(1, 0), "ykGp");
neovim_test!(scenarios, matrix_y_plus, "line1\n  line2\nline3", "y+Gp");
neovim_test!(scenarios, y_minus, "line1\n  line2\nline3", cursor(2, 0), "y-Gp");

// y + document motions (linewise yank)
neovim_test!(scenarios, y_gg, "l1\nl2\nl3\nl4", cursor(2, 0), "yggGp");
neovim_test!(scenarios, y_G, "l1\nl2\nl3\nl4", "yGGp");

// y + paragraph
neovim_test!(scenarios, y_open_brace, "para1\n\npara2", cursor(2, 0), "y{P");
neovim_test!(scenarios, y_close_brace, "para1\n\npara2", "y}Gp");

// y + percent
neovim_test!(scenarios, y_percent_paren, "(hello world)", "y%$p");
neovim_test!(scenarios, y_percent_brace, "{hello world}", "y%$p");

// y + search
neovim_test!(scenarios, y_search_fwd_matrix, "hello world test end", "y/test<CR>$p");
neovim_test!(scenarios, y_search_bwd_matrix, "hello world test end", cursor(0, 17), "y?test<CR>0P");

// ═══════════════════════════════════════════════════════════════════════════════
// INDENT (>) × MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, indent_j, "line1\nline2\nline3", ">j");
neovim_test!(scenarios, indent_k, "line1\nline2\nline3", cursor(1, 0), ">k");
neovim_test!(scenarios, indent_2j, "l1\nl2\nl3\nl4", ">2j");
neovim_test!(scenarios, matrix_indent_gg, "l1\nl2\nl3\nl4", cursor(2, 0), ">gg");
neovim_test!(scenarios, matrix_indent_G, "l1\nl2\nl3\nl4", ">G");
neovim_test!(scenarios, indent_brace, "para1\n\npara2", ">}");
neovim_test!(scenarios, indent_paren, "Hello. World.", ">)");
neovim_test!(scenarios, indent_percent, "(\nhello\n)", ">%");
neovim_test!(scenarios, indent_search, "l1\nl2\nl3", ">/l3<CR>");
neovim_test!(scenarios, indent_plus, "line1\n  line2\nline3", ">+");
neovim_test!(scenarios, indent_minus, "line1\n  line2\nline3", cursor(2, 0), ">-");
neovim_test!(scenarios, indent_iw, "hello world", ">iw");
neovim_test!(scenarios, indent_ip, "line1\nline2\n\nline3", ">ip");

// ═══════════════════════════════════════════════════════════════════════════════
// OUTDENT (<) × MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, outdent_j, "    line1\n    line2\n    line3", "<j");
neovim_test!(scenarios, outdent_k, "    line1\n    line2\n    line3", cursor(1, 0), "<k");
neovim_test!(scenarios, outdent_2j, "    l1\n    l2\n    l3\n    l4", "<2j");
neovim_test!(scenarios, outdent_gg, "    l1\n    l2\n    l3", cursor(2, 0), "<gg");
neovim_test!(scenarios, outdent_G, "    l1\n    l2\n    l3", "<G");
neovim_test!(scenarios, outdent_brace, "    para1\n\n    para2", "<}");
neovim_test!(scenarios, outdent_ip, "    line1\n    line2\n\n    line3", "<ip");
neovim_test!(scenarios, outdent_plus, "    line1\n    line2\nline3", "<+");

// ═══════════════════════════════════════════════════════════════════════════════
// LOWERCASE (gu) × MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, gu_h, "HELLO", cursor(0, 3), "guh");
neovim_test!(scenarios, gu_l, "HELLO", "gul");
neovim_test!(scenarios, gu_w, "HELLO WORLD", "guw");
neovim_test!(scenarios, gu_e, "HELLO WORLD", "gue");
neovim_test!(scenarios, gu_b, "HELLO WORLD", cursor(0, 6), "gub");
neovim_test!(scenarios, gu_W, "FOO.BAR BAZ", "guW");
neovim_test!(scenarios, gu_0, "HELLO WORLD", cursor(0, 5), "gu0");
neovim_test!(scenarios, gu_dollar, "HELLO WORLD", "gu$");
neovim_test!(scenarios, gu_caret, "  HELLO", cursor(0, 6), "gu^");
neovim_test!(scenarios, gu_f_o, "HELLO WORLD", "gufo");
neovim_test!(scenarios, gu_t_o, "HELLO WORLD", "guto");
neovim_test!(scenarios, gu_j, "HELLO\nWORLD", "guj");
neovim_test!(scenarios, gu_G, "HELLO\nWORLD\nTEST", "guG");
neovim_test!(scenarios, gu_gg, "HELLO\nWORLD\nTEST", cursor(2, 0), "gugg");
neovim_test!(scenarios, gu_brace, "HELLO\n\nWORLD", "gu}");
neovim_test!(scenarios, gu_percent, "(HELLO)", "gu%");
neovim_test!(scenarios, gu_iw, "HELLO world", "guiw");
neovim_test!(scenarios, gu_aw, "HELLO WORLD", "guaw");
neovim_test!(scenarios, gu_ip, "HELLO\nWORLD\n\nTEST", "guip");
neovim_test!(scenarios, gu_search, "HELLO world TEST", "gu/TEST<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// UPPERCASE (gU) × MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, gU_h, "hello", cursor(0, 3), "gUh");
neovim_test!(scenarios, gU_l, "hello", "gUl");
neovim_test!(scenarios, gU_w, "hello world", "gUw");
neovim_test!(scenarios, gU_e, "hello world", "gUe");
neovim_test!(scenarios, gU_b, "hello world", cursor(0, 6), "gUb");
neovim_test!(scenarios, gU_W, "foo.bar baz", "gUW");
neovim_test!(scenarios, gU_0, "hello world", cursor(0, 5), "gU0");
neovim_test!(scenarios, gU_dollar, "hello world", "gU$");
neovim_test!(scenarios, gU_caret, "  hello", cursor(0, 6), "gU^");
neovim_test!(scenarios, gU_f_o, "hello world", "gUfo");
neovim_test!(scenarios, gU_t_o, "hello world", "gUto");
neovim_test!(scenarios, gU_j, "hello\nworld", "gUj");
neovim_test!(scenarios, gU_G, "hello\nworld\ntest", "gUG");
neovim_test!(scenarios, gU_gg, "hello\nworld\ntest", cursor(2, 0), "gUgg");
neovim_test!(scenarios, gU_brace, "hello\n\nworld", "gU}");
neovim_test!(scenarios, gU_percent, "(hello)", "gU%");
neovim_test!(scenarios, gU_iw, "hello WORLD", "gUiw");
neovim_test!(scenarios, gU_aw, "hello world", "gUaw");
neovim_test!(scenarios, gU_ip, "hello\nworld\n\ntest", "gUip");
neovim_test!(scenarios, gU_search, "hello WORLD test", "gU/WORLD<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// TOGGLE CASE (g~) × MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, g_tilde_h, "HeLLo", cursor(0, 3), "g~h");
neovim_test!(scenarios, g_tilde_l, "HeLLo", "g~l");
neovim_test!(scenarios, g_tilde_w, "HeLLo WoRLd", "g~w");
neovim_test!(scenarios, g_tilde_e, "HeLLo WoRLd", "g~e");
neovim_test!(scenarios, g_tilde_b, "HeLLo WoRLd", cursor(0, 6), "g~b");
neovim_test!(scenarios, g_tilde_W, "HeL.Lo WoR.Ld", "g~W");
neovim_test!(scenarios, g_tilde_0, "HeLLo", cursor(0, 3), "g~0");
neovim_test!(scenarios, g_tilde_dollar, "HeLLo WoRLd", "g~$");
neovim_test!(scenarios, g_tilde_f_o, "HeLLo WoRLd", "g~fo");
neovim_test!(scenarios, g_tilde_j, "HeLLo\nWoRLd", "g~j");
neovim_test!(scenarios, g_tilde_G, "HeLLo\nWoRLd\nTeSt", "g~G");
neovim_test!(scenarios, g_tilde_gg, "HeLLo\nWoRLd\nTeSt", cursor(2, 0), "g~gg");
neovim_test!(scenarios, g_tilde_brace, "HeLLo\n\nWoRLd", "g~}");
neovim_test!(scenarios, g_tilde_percent, "(HeLLo)", "g~%");
neovim_test!(scenarios, g_tilde_iw, "HeLLo WoRLd", "g~iw");
neovim_test!(scenarios, g_tilde_ip, "HeLLo\nWoRLd\n\nTeSt", "g~ip");

// ═══════════════════════════════════════════════════════════════════════════════
// FORMAT (gq) × MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, gq_j, "line1 text\nline2 text", "gqj");
neovim_test!(scenarios, gq_G, "line1\nline2\nline3", "gqG");
neovim_test!(scenarios, gq_gg, "line1\nline2\nline3", cursor(2, 0), "gqgg");
neovim_test!(scenarios, gq_brace, "line1\nline2\n\nline3", "gq}");
neovim_test!(scenarios, gq_ip, "line1\nline2\n\nline3", "gqip");
neovim_test!(scenarios, gq_ap, "line1\nline2\n\nline3", "gqap");
neovim_test!(scenarios, gq_w, "hello world test", "gqw");
neovim_test!(scenarios, gq_dollar, "hello world test", "gq$");
neovim_test!(scenarios, gq_plus, "line1\n  line2\nline3", "gq+");

// ═══════════════════════════════════════════════════════════════════════════════
// EQUAL/AUTOINDENT (=) × MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, eq_j, "  line1\nline2\n  line3", "=j");
neovim_test!(scenarios, eq_G, "  line1\nline2\n  line3", "=G");
neovim_test!(scenarios, eq_gg, "  line1\nline2\n  line3", cursor(2, 0), "=gg");
neovim_test!(scenarios, eq_brace, "  line1\nline2\n\nline3", "=}");
neovim_test!(scenarios, eq_ip, "  line1\nline2\n\nline3", "=ip");
neovim_test!(scenarios, eq_percent, "(\nhello\n)", "=%");
neovim_test!(scenarios, eq_plus, "  line1\nline2\nline3", "=+");
neovim_test!(scenarios, eq_k, "  line1\nline2\n  line3", cursor(1, 0), "=k");

// ═══════════════════════════════════════════════════════════════════════════════
// DOUBLED OPERATORS (dd, cc, yy, >>, <<, guu, gUU, g~~, gqq, ==)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dd_basic, "l1\nl2\nl3", cursor(1, 0), "dd");
neovim_test!(scenarios, cc_basic, "l1\nl2\nl3", cursor(1, 0), "ccX<Esc>");
neovim_test!(scenarios, yy_basic, "l1\nl2\nl3", cursor(1, 0), "yyGp");
neovim_test!(scenarios, indent_indent, "l1\nl2\nl3", ">>");
neovim_test!(scenarios, outdent_outdent, "    l1\n    l2\n    l3", "<<");
neovim_test!(scenarios, guu_basic, "HELLO WORLD", "guu");
neovim_test!(scenarios, gUU_basic, "hello world", "gUU");
neovim_test!(scenarios, g_tilde_tilde, "HeLLo WoRLd", "g~~");
neovim_test!(scenarios, matrix_gqq_basic, "hello world test line", "gqq");
neovim_test!(scenarios, eq_eq, "  hello world", "==");

// Doubled with counts
neovim_test!(scenarios, matrix_2dd, "l1\nl2\nl3\nl4", "2dd");
neovim_test!(scenarios, matrix_3cc, "l1\nl2\nl3\nl4\nl5", "3ccX<Esc>");
neovim_test!(scenarios, matrix_2yy, "l1\nl2\nl3\nl4", "2yyGp");
neovim_test!(scenarios, matrix_3_indent, "l1\nl2\nl3\nl4", "3>>");
neovim_test!(scenarios, matrix_3_outdent, "    l1\n    l2\n    l3\n    l4", "3<<");
neovim_test!(scenarios, matrix_2guu, "HELLO\nWORLD\nTEST", "2guu");
neovim_test!(scenarios, matrix_2gUU, "hello\nworld\ntest", "2gUU");
neovim_test!(scenarios, matrix_2g_tilde_tilde, "HeLLo\nWoRLd\nTeSt", "2g~~");
neovim_test!(scenarios, matrix_2gqq, "line1\nline2\nline3", "2gqq");
neovim_test!(scenarios, matrix_2eq_eq, "  l1\n  l2\n  l3", "2==");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATORS WITH TEXT OBJECTS (COMPREHENSIVE)
// ═══════════════════════════════════════════════════════════════════════════════

// d + all text objects
neovim_test!(scenarios, d_iw, "hello world", cursor(0, 2), "diw");
neovim_test!(scenarios, d_aw, "hello world test", cursor(0, 6), "daw");
neovim_test!(scenarios, d_iW, "foo.bar baz", cursor(0, 2), "diW");
neovim_test!(scenarios, d_aW, "foo.bar baz", cursor(0, 2), "daW");
neovim_test!(scenarios, d_is, "Hello. World. End.", cursor(0, 7), "dis");
neovim_test!(scenarios, d_as, "Hello. World. End.", cursor(0, 7), "das");
neovim_test!(scenarios, d_ip, "para1\n\npara2\n\npara3", cursor(2, 0), "dip");
neovim_test!(scenarios, d_ap, "para1\n\npara2\n\npara3", cursor(2, 0), "dap");
neovim_test!(scenarios, d_i_paren, "(hello world)", cursor(0, 5), "di(");
neovim_test!(scenarios, d_a_paren, "(hello world)", cursor(0, 5), "da(");
neovim_test!(scenarios, d_i_brace, "{hello world}", cursor(0, 5), "di{");
neovim_test!(scenarios, d_a_brace, "{hello world}", cursor(0, 5), "da{");
neovim_test!(scenarios, d_i_bracket, "[hello world]", cursor(0, 5), "di[");
neovim_test!(scenarios, d_a_bracket, "[hello world]", cursor(0, 5), "da[");
neovim_test!(scenarios, d_i_dquote, "\"hello world\"", cursor(0, 5), "di\"");
neovim_test!(scenarios, d_a_dquote, "\"hello world\"", cursor(0, 5), "da\"");
neovim_test!(scenarios, d_i_squote, "'hello world'", cursor(0, 5), "di'");
neovim_test!(scenarios, d_a_squote, "'hello world'", cursor(0, 5), "da'");
neovim_test!(scenarios, d_i_backtick, "`hello world`", cursor(0, 5), "di`");
neovim_test!(scenarios, d_a_backtick, "`hello world`", cursor(0, 5), "da`");
neovim_test!(scenarios, d_it, "<div>hello</div>", cursor(0, 7), "dit");
neovim_test!(scenarios, d_at, "<div>hello</div>", cursor(0, 7), "dat");

// c + all text objects
neovim_test!(scenarios, c_iw, "hello world", cursor(0, 2), "ciwX<Esc>");
neovim_test!(scenarios, c_aw, "hello world test", cursor(0, 6), "cawX<Esc>");
neovim_test!(scenarios, c_iW, "foo.bar baz", cursor(0, 2), "ciWX<Esc>");
neovim_test!(scenarios, c_aW, "foo.bar baz", cursor(0, 2), "caWX<Esc>");
neovim_test!(scenarios, c_is, "Hello. World. End.", cursor(0, 7), "cisX<Esc>");
neovim_test!(scenarios, c_as, "Hello. World. End.", cursor(0, 7), "casX<Esc>");
neovim_test!(scenarios, c_ip, "para1\n\npara2", "cipX<Esc>");
neovim_test!(scenarios, c_ap, "para1\n\npara2", "capX<Esc>");
neovim_test!(scenarios, c_i_paren, "(hello)", cursor(0, 3), "ci(X<Esc>");
neovim_test!(scenarios, c_a_paren, "(hello)", cursor(0, 3), "ca(X<Esc>");
neovim_test!(scenarios, c_i_brace, "{hello}", cursor(0, 3), "ci{X<Esc>");
neovim_test!(scenarios, c_a_brace, "{hello}", cursor(0, 3), "ca{X<Esc>");
neovim_test!(scenarios, c_i_bracket, "[hello]", cursor(0, 3), "ci[X<Esc>");
neovim_test!(scenarios, c_a_bracket, "[hello]", cursor(0, 3), "ca[X<Esc>");
neovim_test!(scenarios, c_i_dquote, "\"hello\"", cursor(0, 3), "ci\"X<Esc>");
neovim_test!(scenarios, c_a_dquote, "\"hello\"", cursor(0, 3), "ca\"X<Esc>");
neovim_test!(scenarios, c_i_squote, "'hello'", cursor(0, 3), "ci'X<Esc>");
neovim_test!(scenarios, c_a_squote, "'hello'", cursor(0, 3), "ca'X<Esc>");
neovim_test!(scenarios, c_i_backtick, "`hello`", cursor(0, 3), "ci`X<Esc>");
neovim_test!(scenarios, c_a_backtick, "`hello`", cursor(0, 3), "ca`X<Esc>");
neovim_test!(scenarios, c_it, "<div>hello</div>", cursor(0, 7), "citX<Esc>");
neovim_test!(scenarios, c_at, "<div>hello</div>", cursor(0, 7), "catX<Esc>");

// y + all text objects (verify with paste)
neovim_test!(scenarios, y_iw, "hello world", cursor(0, 2), "yiw$p");
neovim_test!(scenarios, y_aw, "hello world", cursor(0, 2), "yaw$p");
neovim_test!(scenarios, y_i_paren, "(hello)", cursor(0, 3), "yi($p");
neovim_test!(scenarios, y_a_paren, "(hello)", cursor(0, 3), "ya($p");
neovim_test!(scenarios, y_i_brace, "{hello}", cursor(0, 3), "yi{$p");
neovim_test!(scenarios, y_a_brace, "{hello}", cursor(0, 3), "ya{$p");
neovim_test!(scenarios, y_i_dquote, "\"hello\"", cursor(0, 3), "yi\"$p");
neovim_test!(scenarios, y_a_dquote, "\"hello\"", cursor(0, 3), "ya\"$p");
neovim_test!(scenarios, y_i_squote, "'hello'", cursor(0, 3), "yi'$p");
neovim_test!(scenarios, y_a_squote, "'hello'", cursor(0, 3), "ya'$p");
neovim_test!(scenarios, y_i_backtick, "`hello`", cursor(0, 3), "yi`$p");
neovim_test!(scenarios, y_a_backtick, "`hello`", cursor(0, 3), "ya`$p");
neovim_test!(scenarios, y_it, "<div>hello</div>", cursor(0, 7), "yit$p");
neovim_test!(scenarios, y_at, "<div>hello</div>", cursor(0, 7), "yat$p");
neovim_test!(scenarios, y_ip, "para1\n\npara2", "yipGp");
neovim_test!(scenarios, y_ap, "para1\n\npara2", "yapGp");
neovim_test!(scenarios, y_is, "Hello. World.", "yis$p");
neovim_test!(scenarios, y_as, "Hello. World.", "yas$p");

// gu/gU/g~ + text objects
neovim_test!(scenarios, gu_i_paren, "(HELLO)", cursor(0, 3), "gui(");
neovim_test!(scenarios, gu_a_paren, "(HELLO)", cursor(0, 3), "gua(");
neovim_test!(scenarios, gu_i_dquote, "\"HELLO\"", cursor(0, 3), "gui\"");
neovim_test!(scenarios, gu_i_brace, "{HELLO}", cursor(0, 3), "gui{");
neovim_test!(scenarios, gU_i_paren, "(hello)", cursor(0, 3), "gUi(");
neovim_test!(scenarios, gU_a_paren, "(hello)", cursor(0, 3), "gUa(");
neovim_test!(scenarios, gU_i_dquote, "\"hello\"", cursor(0, 3), "gUi\"");
neovim_test!(scenarios, gU_i_brace, "{hello}", cursor(0, 3), "gUi{");
neovim_test!(scenarios, g_tilde_i_paren, "(HeLLo)", cursor(0, 3), "g~i(");
neovim_test!(scenarios, g_tilde_i_dquote, "\"HeLLo\"", cursor(0, 3), "g~i\"");

// > and < with text objects
neovim_test!(scenarios, indent_ip_op, "line1\nline2\n\nline3", ">ip");
neovim_test!(scenarios, indent_ap_op, "line1\nline2\n\nline3", ">ap");
neovim_test!(scenarios, outdent_ip_op, "    line1\n    line2\n\nline3", "<ip");
neovim_test!(scenarios, outdent_ap_op, "    line1\n    line2\n\nline3", "<ap");
neovim_test!(scenarios, indent_i_brace, "{\nhello\nworld\n}", cursor(1, 0), ">i{");
neovim_test!(scenarios, outdent_i_brace, "{\n    hello\n    world\n}", cursor(1, 0), "<i{");

// = with text objects
neovim_test!(scenarios, eq_ip_op, "  line1\nline2\n\nline3", "=ip");
neovim_test!(scenarios, eq_i_brace, "{\n  hello\nworld\n}", cursor(1, 0), "=i{");
neovim_test!(scenarios, eq_it, "<div>\n  hello\nworld\n</div>", cursor(1, 0), "=it");

// gq with text objects
neovim_test!(scenarios, gq_ip_op, "line1\nline2\n\nline3", "gqip");
neovim_test!(scenarios, gq_ap_op, "line1\nline2\n\nline3", "gqap");
neovim_test!(scenarios, gq_is, "Hello world. Next sentence.", "gqis");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATORS × PIPE MOTION (|)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, d_pipe_5, "hello world test", "d5|");
neovim_test!(scenarios, c_pipe_5, "hello world test", "c5|X<Esc>");
neovim_test!(scenarios, y_pipe_5, "hello world test", "y5|$p");
neovim_test!(scenarios, gu_pipe_5, "HELLO WORLD", "gu5|");
neovim_test!(scenarios, gU_pipe_5, "hello world", "gU5|");
neovim_test!(scenarios, g_tilde_pipe, "HeLLo", "g~5|");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATORS × UNDERSCORE MOTION (_)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, matrix_d_underscore, "  hello world", "d_");
neovim_test!(scenarios, matrix_c_underscore, "  hello world", "c_X<Esc>");
neovim_test!(scenarios, y_underscore, "  hello world", "y_Gp");
neovim_test!(scenarios, gu_underscore, "  HELLO WORLD", "gu_");
neovim_test!(scenarios, gU_underscore, "  hello world", "gU_");
neovim_test!(scenarios, d_2_underscore, "l1\n  l2\nl3", "d2_");
neovim_test!(scenarios, c_2_underscore, "l1\n  l2\nl3", "c2_X<Esc>");
neovim_test!(scenarios, y_2_underscore, "l1\n  l2\nl3", "y2_Gp");

// cF/cT from $ (end of line) — verifies cursor char excluded from change range
neovim_test!(scenarios, cF_from_dollar, "abcdefgh", cursor(0, 7), "cFdXX<Esc>");
neovim_test!(scenarios, cT_from_dollar, "abcdefgh", cursor(0, 7), "cTdXX<Esc>");
