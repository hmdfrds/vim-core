// Empty Buffer and Single Character Stress Tests.
//
// Every normal-mode command tested on:
//   1. Empty buffer ("")
//   2. Single character buffer ("a")
//   3. Single newline buffer ("\n")
//
// These tests catch crashes, panics, and off-by-one errors.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC MOTIONS — EMPTY BUFFER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, empty_h, "", "h");
neovim_test!(scenarios, empty_l, "", "l");
neovim_test!(scenarios, empty_j, "", "j");
neovim_test!(scenarios, empty_k, "", "k");
neovim_test!(scenarios, empty_w, "", "w");
neovim_test!(scenarios, empty_e, "", "e");
neovim_test!(scenarios, empty_b, "", "b");
neovim_test!(scenarios, empty_W, "", "W");
neovim_test!(scenarios, empty_E, "", "E");
neovim_test!(scenarios, empty_B, "", "B");
neovim_test!(scenarios, empty_ge, "", "ge");
neovim_test!(scenarios, empty_gE, "", "gE");
neovim_test!(scenarios, empty_0, "", "0");
neovim_test!(scenarios, empty_dollar, "", "$");
neovim_test!(scenarios, empty_caret, "", "^");
neovim_test!(scenarios, empty_g_underscore, "", "g_");
neovim_test!(scenarios, empty_gg, "", "gg");
neovim_test!(scenarios, empty_G, "", "G");
neovim_test!(scenarios, empty_percent, "", "%");
neovim_test!(scenarios, empty_pipe, "", "5|");
neovim_test!(scenarios, empty_underscore, "", "_");
neovim_test!(scenarios, empty_plus, "", "+");
neovim_test!(scenarios, empty_minus, "", "-");
neovim_test!(scenarios, empty_H, "", "H");
neovim_test!(scenarios, empty_M, "", "M");
neovim_test!(scenarios, empty_L, "", "L");
neovim_test!(scenarios, empty_f, "", "fa");
neovim_test!(scenarios, empty_F, "", "Fa");
neovim_test!(scenarios, empty_t, "", "ta");
neovim_test!(scenarios, empty_T, "", "Ta");
neovim_test!(scenarios, empty_semicolon, "", ";");
neovim_test!(scenarios, empty_comma, "", ",");
neovim_test!(scenarios, empty_open_brace, "", "{");
neovim_test!(scenarios, empty_close_brace, "", "}");
neovim_test!(scenarios, empty_open_paren, "", "(");
neovim_test!(scenarios, empty_close_paren, "", ")");

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC MOTIONS — SINGLE CHAR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, single_h, "a", "h");
neovim_test!(scenarios, single_l, "a", "l");
neovim_test!(scenarios, single_j, "a", "j");
neovim_test!(scenarios, single_k, "a", "k");
neovim_test!(scenarios, single_w, "a", "w");
neovim_test!(scenarios, single_e, "a", "e");
neovim_test!(scenarios, single_b, "a", "b");
neovim_test!(scenarios, single_W, "a", "W");
neovim_test!(scenarios, single_E, "a", "E");
neovim_test!(scenarios, single_B, "a", "B");
neovim_test!(scenarios, single_ge, "a", "ge");
neovim_test!(scenarios, single_gE, "a", "gE");
neovim_test!(scenarios, single_0, "a", "0");
neovim_test!(scenarios, single_dollar, "a", "$");
neovim_test!(scenarios, single_caret, "a", "^");
neovim_test!(scenarios, single_g_underscore, "a", "g_");
neovim_test!(scenarios, single_gg, "a", "gg");
neovim_test!(scenarios, single_G, "a", "G");
neovim_test!(scenarios, single_percent, "a", "%");
neovim_test!(scenarios, single_pipe, "a", "1|");
neovim_test!(scenarios, single_underscore, "a", "_");
neovim_test!(scenarios, single_plus, "a", "+");
neovim_test!(scenarios, single_minus, "a", "-");
neovim_test!(scenarios, single_H, "a", "H");
neovim_test!(scenarios, single_M, "a", "M");
neovim_test!(scenarios, single_L, "a", "L");
neovim_test!(scenarios, single_f, "a", "fa");
neovim_test!(scenarios, single_F, "a", "Fa");
neovim_test!(scenarios, single_t, "a", "ta");
neovim_test!(scenarios, single_T, "a", "Ta");
neovim_test!(scenarios, single_semicolon, "a", ";");
neovim_test!(scenarios, single_comma, "a", ",");
neovim_test!(scenarios, single_open_brace, "a", "{");
neovim_test!(scenarios, single_close_brace, "a", "}");
neovim_test!(scenarios, single_open_paren, "a", "(");
neovim_test!(scenarios, single_close_paren, "a", ")");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATORS — EMPTY BUFFER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, empty_x, "", "x");
neovim_test!(scenarios, empty_X, "", "X");
neovim_test!(scenarios, empty_dd, "", "dd");
neovim_test!(scenarios, empty_dw, "", "dw");
neovim_test!(scenarios, empty_de, "", "de");
neovim_test!(scenarios, empty_db, "", "db");
neovim_test!(scenarios, empty_d0, "", "d0");
neovim_test!(scenarios, empty_d_dollar, "", "d$");
neovim_test!(scenarios, empty_d_caret, "", "d^");
neovim_test!(scenarios, empty_D, "", "D");
neovim_test!(scenarios, empty_cc, "", "ccX<Esc>");
neovim_test!(scenarios, empty_cw, "", "cwX<Esc>");
neovim_test!(scenarios, empty_ce, "", "ceX<Esc>");
neovim_test!(scenarios, empty_cb, "", "cbX<Esc>");
neovim_test!(scenarios, empty_c0, "", "c0X<Esc>");
neovim_test!(scenarios, empty_c_dollar, "", "c$X<Esc>");
neovim_test!(scenarios, empty_C, "", "CX<Esc>");
neovim_test!(scenarios, empty_yy, "", "yy");
neovim_test!(scenarios, empty_yw, "", "yw");
neovim_test!(scenarios, empty_ye, "", "ye");
neovim_test!(scenarios, empty_yb, "", "yb");
neovim_test!(scenarios, empty_y_dollar, "", "y$");
neovim_test!(scenarios, empty_Y, "", "Y");
neovim_test!(scenarios, empty_s, "", "sX<Esc>");
neovim_test!(scenarios, empty_S, "", "SX<Esc>");
neovim_test!(scenarios, empty_r, "", "ra");
neovim_test!(scenarios, empty_J, "", "J");
neovim_test!(scenarios, empty_gJ, "", "gJ");
neovim_test!(scenarios, empty_p, "", "p");
neovim_test!(scenarios, empty_P, "", "P");
neovim_test!(scenarios, empty_indent, "", ">>");
neovim_test!(scenarios, empty_outdent, "", "<<");
neovim_test!(scenarios, empty_guu, "", "guu");
neovim_test!(scenarios, empty_gUU, "", "gUU");
neovim_test!(scenarios, empty_g_tilde_tilde, "", "g~~");
neovim_test!(scenarios, empty_tilde, "", "~");
neovim_test!(scenarios, empty_gqq, "", "gqq");
neovim_test!(scenarios, empty_eq_eq, "", "==");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATORS — SINGLE CHAR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, single_x, "a", "x");
neovim_test!(scenarios, single_X, "a", "X");
neovim_test!(scenarios, single_dd, "a", "dd");
neovim_test!(scenarios, single_dw, "a", "dw");
neovim_test!(scenarios, single_de, "a", "de");
neovim_test!(scenarios, single_db, "a", "db");
neovim_test!(scenarios, single_d0, "a", "d0");
neovim_test!(scenarios, single_d_dollar, "a", "d$");
neovim_test!(scenarios, single_d_caret, "a", "d^");
neovim_test!(scenarios, single_D, "a", "D");
neovim_test!(scenarios, single_cc, "a", "ccX<Esc>");
neovim_test!(scenarios, single_cw, "a", "cwX<Esc>");
neovim_test!(scenarios, single_ce, "a", "ceX<Esc>");
neovim_test!(scenarios, single_cb, "a", "cbX<Esc>");
neovim_test!(scenarios, single_c_dollar, "a", "c$X<Esc>");
neovim_test!(scenarios, single_C, "a", "CX<Esc>");
neovim_test!(scenarios, single_yy, "a", "yy");
neovim_test!(scenarios, single_yw, "a", "yw");
neovim_test!(scenarios, single_ye, "a", "ye");
neovim_test!(scenarios, single_y_dollar, "a", "y$");
neovim_test!(scenarios, single_Y, "a", "Y");
neovim_test!(scenarios, single_s, "a", "sX<Esc>");
neovim_test!(scenarios, single_S, "a", "SX<Esc>");
neovim_test!(scenarios, single_r, "a", "rX");
neovim_test!(scenarios, single_J, "a", "J");
neovim_test!(scenarios, single_gJ, "a", "gJ");
neovim_test!(scenarios, single_p, "a", "p");
neovim_test!(scenarios, single_P, "a", "P");
neovim_test!(scenarios, single_indent, "a", ">>");
neovim_test!(scenarios, single_outdent, "a", "<<");
neovim_test!(scenarios, single_guu, "A", "guu");
neovim_test!(scenarios, single_gUU, "a", "gUU");
neovim_test!(scenarios, single_g_tilde_tilde, "a", "g~~");
neovim_test!(scenarios, single_tilde, "a", "~");
neovim_test!(scenarios, single_gqq, "a", "gqq");
neovim_test!(scenarios, single_eq_eq, "a", "==");

// ═══════════════════════════════════════════════════════════════════════════════
// TEXT OBJECTS — EMPTY BUFFER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, empty_diw, "", "diw");
neovim_test!(scenarios, empty_daw, "", "daw");
neovim_test!(scenarios, empty_diW, "", "diW");
neovim_test!(scenarios, empty_daW, "", "daW");
neovim_test!(scenarios, empty_dis, "", "dis");
neovim_test!(scenarios, empty_das, "", "das");
neovim_test!(scenarios, empty_dip, "", "dip");
neovim_test!(scenarios, empty_dap, "", "dap");
neovim_test!(scenarios, empty_di_paren, "", "di(");
neovim_test!(scenarios, empty_da_paren, "", "da(");
neovim_test!(scenarios, empty_di_brace, "", "di{");
neovim_test!(scenarios, empty_da_brace, "", "da{");
neovim_test!(scenarios, empty_di_bracket, "", "di[");
neovim_test!(scenarios, empty_da_bracket, "", "da[");
neovim_test!(scenarios, empty_di_dquote, "", "di\"");
neovim_test!(scenarios, empty_da_dquote, "", "da\"");
neovim_test!(scenarios, empty_di_squote, "", "di'");
neovim_test!(scenarios, empty_da_squote, "", "da'");
neovim_test!(scenarios, empty_di_backtick, "", "di`");
neovim_test!(scenarios, empty_da_backtick, "", "da`");
neovim_test!(scenarios, empty_dit, "", "dit");
neovim_test!(scenarios, empty_dat, "", "dat");

// ═══════════════════════════════════════════════════════════════════════════════
// TEXT OBJECTS — SINGLE CHAR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, single_diw, "a", "diw");
neovim_test!(scenarios, single_daw, "a", "daw");
neovim_test!(scenarios, single_diW, "a", "diW");
neovim_test!(scenarios, single_daW, "a", "daW");
neovim_test!(scenarios, single_dis, "a", "dis");
neovim_test!(scenarios, single_das, "a", "das");
neovim_test!(scenarios, single_dip, "a", "dip");
neovim_test!(scenarios, single_dap, "a", "dap");
neovim_test!(scenarios, single_di_paren, "a", "di(");
neovim_test!(scenarios, single_da_paren, "a", "da(");
neovim_test!(scenarios, single_di_brace, "a", "di{");
neovim_test!(scenarios, single_da_brace, "a", "da{");
neovim_test!(scenarios, single_di_bracket, "a", "di[");
neovim_test!(scenarios, single_da_bracket, "a", "da[");
neovim_test!(scenarios, single_di_dquote, "a", "di\"");
neovim_test!(scenarios, single_da_dquote, "a", "da\"");
neovim_test!(scenarios, single_di_squote, "a", "di'");
neovim_test!(scenarios, single_da_squote, "a", "da'");
neovim_test!(scenarios, single_di_backtick, "a", "di`");
neovim_test!(scenarios, single_da_backtick, "a", "da`");
neovim_test!(scenarios, single_dit, "a", "dit");
neovim_test!(scenarios, single_dat, "a", "dat");

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT MODE — EMPTY BUFFER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, empty_i, "", "i<Esc>");
neovim_test!(scenarios, empty_a, "", "a<Esc>");
neovim_test!(scenarios, empty_I, "", "I<Esc>");
neovim_test!(scenarios, empty_A, "", "A<Esc>");
neovim_test!(scenarios, empty_o, "", "oX<Esc>");
neovim_test!(scenarios, empty_O, "", "OX<Esc>");
neovim_test!(scenarios, empty_i_text, "", "ihello<Esc>");
neovim_test!(scenarios, empty_a_text, "", "ahello<Esc>");
neovim_test!(scenarios, empty_I_text, "", "Ihello<Esc>");
neovim_test!(scenarios, empty_A_text, "", "Ahello<Esc>");
neovim_test!(scenarios, empty_o_text, "", "ohello<Esc>");
neovim_test!(scenarios, empty_O_text, "", "Ohello<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT MODE — SINGLE CHAR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, single_i, "a", "i<Esc>");
neovim_test!(scenarios, single_a_insert, "a", "a<Esc>");
neovim_test!(scenarios, single_I, "a", "I<Esc>");
neovim_test!(scenarios, single_A_insert, "a", "A<Esc>");
neovim_test!(scenarios, single_o, "a", "oX<Esc>");
neovim_test!(scenarios, single_O, "a", "OX<Esc>");
neovim_test!(scenarios, single_i_text, "a", "iX<Esc>");
neovim_test!(scenarios, single_a_text, "a", "aX<Esc>");
neovim_test!(scenarios, single_I_text, "a", "IX<Esc>");
neovim_test!(scenarios, single_A_text, "a", "AX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL MODE — EMPTY BUFFER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, empty_v, "", "v<Esc>");
neovim_test!(scenarios, empty_V, "", "V<Esc>");
neovim_test!(scenarios, empty_ctrl_v, "", "<C-v><Esc>");
neovim_test!(scenarios, empty_vd, "", "vd");
neovim_test!(scenarios, empty_Vd, "", "Vd");
neovim_test!(scenarios, empty_vy, "", "vy");
neovim_test!(scenarios, empty_Vy, "", "Vy");
neovim_test!(scenarios, empty_vc, "", "vcX<Esc>");
neovim_test!(scenarios, empty_Vc, "", "VcX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL MODE — SINGLE CHAR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, single_v, "a", "v<Esc>");
neovim_test!(scenarios, single_V, "a", "V<Esc>");
neovim_test!(scenarios, single_ctrl_v, "a", "<C-v><Esc>");
neovim_test!(scenarios, single_vd, "a", "vd");
neovim_test!(scenarios, single_Vd, "a", "Vd");
neovim_test!(scenarios, single_vy, "a", "vy");
neovim_test!(scenarios, single_Vy, "a", "Vy");
neovim_test!(scenarios, single_vc, "a", "vcX<Esc>");
neovim_test!(scenarios, single_Vc, "a", "VcX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO/REDO — EMPTY BUFFER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, empty_u, "", "u");
neovim_test!(scenarios, empty_ctrl_r, "", "<C-r>");
neovim_test!(scenarios, empty_dot, "", ".");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO/REDO — SINGLE CHAR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, single_u, "a", "u");
neovim_test!(scenarios, single_ctrl_r, "a", "<C-r>");
neovim_test!(scenarios, single_dot, "a", ".");
neovim_test!(scenarios, single_x_u, "a", "xu");
neovim_test!(scenarios, single_x_ctrl_r, "a", "xu<C-r>");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH — EMPTY BUFFER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, empty_search_fwd, "", "/test<CR>");
neovim_test!(scenarios, empty_search_bwd, "", "?test<CR>");
neovim_test!(scenarios, empty_star, "", "*");
neovim_test!(scenarios, empty_hash, "", "#");
neovim_test!(scenarios, empty_n, "", "n");
neovim_test!(scenarios, empty_N, "", "N");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH — SINGLE CHAR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, single_search_fwd, "a", "/a<CR>");
neovim_test!(scenarios, single_search_bwd, "a", "?a<CR>");
neovim_test!(scenarios, single_star, "a", "*");
neovim_test!(scenarios, single_hash, "a", "#");
neovim_test!(scenarios, single_n, "a", "/a<CR>n");
neovim_test!(scenarios, single_N, "a", "/a<CR>N");

// ═══════════════════════════════════════════════════════════════════════════════
// MACROS — EMPTY BUFFER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, empty_macro_record, "", "qaq");
neovim_test!(scenarios, empty_macro_play, "", "@a");
neovim_test!(scenarios, empty_macro_repeat, "", "@@");

// ═══════════════════════════════════════════════════════════════════════════════
// MACROS — SINGLE CHAR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, single_macro_record, "a", "qaxq");
neovim_test!(scenarios, single_macro_play, "a", "qaxq@a");
neovim_test!(scenarios, single_macro_repeat, "a", "qaxq@a@@");

// ═══════════════════════════════════════════════════════════════════════════════
// MARKS — EMPTY BUFFER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, empty_mark_set, "", "ma");
neovim_test!(scenarios, empty_mark_jump, "", "'a");
neovim_test!(scenarios, empty_mark_backtick, "", "`a");

// ═══════════════════════════════════════════════════════════════════════════════
// MARKS — SINGLE CHAR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, single_mark_set, "a", "ma");
neovim_test!(scenarios, single_mark_jump, "a", "ma'a");
neovim_test!(scenarios, single_mark_backtick, "a", "ma`a");

// ═══════════════════════════════════════════════════════════════════════════════
// SCROLL — EMPTY BUFFER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, empty_ctrl_d, "", "<C-d>");
neovim_test!(scenarios, empty_ctrl_u, "", "<C-u>");
neovim_test!(scenarios, empty_ctrl_f, "", "<C-f>");
neovim_test!(scenarios, empty_ctrl_b, "", "<C-b>");
neovim_test!(scenarios, empty_ctrl_e, "", "<C-e>");
neovim_test!(scenarios, empty_ctrl_y, "", "<C-y>");
neovim_test!(scenarios, empty_zz, "", "zz");
neovim_test!(scenarios, empty_zt, "", "zt");
neovim_test!(scenarios, empty_zb, "", "zb");

// ═══════════════════════════════════════════════════════════════════════════════
// SCROLL — SINGLE CHAR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, single_ctrl_d, "a", "<C-d>");
neovim_test!(scenarios, single_ctrl_u, "a", "<C-u>");
neovim_test!(scenarios, single_ctrl_f, "a", "<C-f>");
neovim_test!(scenarios, single_ctrl_b, "a", "<C-b>");
neovim_test!(scenarios, single_ctrl_e, "a", "<C-e>");
neovim_test!(scenarios, single_ctrl_y, "a", "<C-y>");
neovim_test!(scenarios, single_zz, "a", "zz");
neovim_test!(scenarios, single_zt, "a", "zt");
neovim_test!(scenarios, single_zb, "a", "zb");

// ═══════════════════════════════════════════════════════════════════════════════
// MISC COMMANDS — EMPTY BUFFER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, empty_ctrl_a, "", "<C-a>");
neovim_test!(scenarios, empty_ctrl_x, "", "<C-x>");
neovim_test!(scenarios, empty_gv, "", "gv");
neovim_test!(scenarios, empty_R, "", "R<Esc>");
neovim_test!(scenarios, empty_R_text, "", "RX<Esc>");
neovim_test!(scenarios, empty_ZZ, "", "ZZ");
neovim_test!(scenarios, empty_ZQ, "", "ZQ");

// ═══════════════════════════════════════════════════════════════════════════════
// MISC COMMANDS — SINGLE CHAR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, single_ctrl_a, "5", "<C-a>");
neovim_test!(scenarios, single_ctrl_x, "5", "<C-x>");
neovim_test!(scenarios, single_gv, "a", "gv");
neovim_test!(scenarios, single_R, "a", "R<Esc>");
neovim_test!(scenarios, single_R_text, "a", "RX<Esc>");
neovim_test!(scenarios, single_ZZ, "a", "ZZ");
neovim_test!(scenarios, single_ZQ, "a", "ZQ");

// ═══════════════════════════════════════════════════════════════════════════════
// SINGLE NEWLINE BUFFER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, newline_h, "\n", "h");
neovim_test!(scenarios, newline_l, "\n", "l");
neovim_test!(scenarios, newline_j, "\n", "j");
neovim_test!(scenarios, newline_k, "\n", "k");
neovim_test!(scenarios, newline_w, "\n", "w");
neovim_test!(scenarios, newline_e, "\n", "e");
neovim_test!(scenarios, newline_b, "\n", "b");
neovim_test!(scenarios, newline_x, "\n", "x");
neovim_test!(scenarios, newline_dd, "\n", "dd");
neovim_test!(scenarios, newline_dw, "\n", "dw");
neovim_test!(scenarios, newline_cc, "\n", "ccX<Esc>");
neovim_test!(scenarios, newline_yy, "\n", "yy");
neovim_test!(scenarios, newline_p, "\n", "p");
neovim_test!(scenarios, newline_P, "\n", "P");
neovim_test!(scenarios, newline_o, "\n", "oX<Esc>");
neovim_test!(scenarios, newline_O, "\n", "OX<Esc>");
neovim_test!(scenarios, newline_i, "\n", "iX<Esc>");
neovim_test!(scenarios, newline_a, "\n", "aX<Esc>");
neovim_test!(scenarios, newline_v, "\n", "v<Esc>");
neovim_test!(scenarios, newline_V, "\n", "V<Esc>");
neovim_test!(scenarios, newline_diw, "\n", "diw");
neovim_test!(scenarios, newline_daw, "\n", "daw");
neovim_test!(scenarios, newline_dip, "\n", "dip");
neovim_test!(scenarios, newline_dap, "\n", "dap");
neovim_test!(scenarios, newline_J, "\n", "J");
neovim_test!(scenarios, newline_gJ, "\n", "gJ");
neovim_test!(scenarios, newline_u, "\n", "u");
neovim_test!(scenarios, newline_dot, "\n", ".");
neovim_test!(scenarios, newline_tilde, "\n", "~");
neovim_test!(scenarios, newline_indent, "\n", ">>");
neovim_test!(scenarios, newline_outdent, "\n", "<<");
neovim_test!(scenarios, newline_guu, "\n", "guu");
neovim_test!(scenarios, newline_gUU, "\n", "gUU");
neovim_test!(scenarios, newline_search, "\n", "/test<CR>");
neovim_test!(scenarios, newline_r, "\n", "ra");

// ═══════════════════════════════════════════════════════════════════════════════
// EX COMMANDS — EMPTY BUFFER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, empty_ex_d, "", ":d<CR>");
neovim_test!(scenarios, empty_ex_y, "", ":y<CR>");
neovim_test!(scenarios, empty_ex_s, "", ":s/a/b<CR>");
neovim_test!(scenarios, empty_ex_sort, "", ":%sort<CR>");
neovim_test!(scenarios, empty_ex_j, "", ":j<CR>");
neovim_test!(scenarios, empty_ex_noh, "", ":noh<CR>");
neovim_test!(scenarios, empty_ex_norm, "", ":%norm A!<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// EX COMMANDS — SINGLE CHAR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, single_ex_d, "a", ":d<CR>");
neovim_test!(scenarios, single_ex_y, "a", ":y<CR>p");
neovim_test!(scenarios, single_ex_s, "a", ":s/a/b<CR>");
neovim_test!(scenarios, single_ex_sort, "a", ":%sort<CR>");
neovim_test!(scenarios, single_ex_j, "a", ":j<CR>");
neovim_test!(scenarios, single_ex_noh, "a", ":noh<CR>");
neovim_test!(scenarios, single_ex_norm, "a", ":%norm A!<CR>");
