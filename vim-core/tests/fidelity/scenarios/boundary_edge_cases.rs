// Boundary edge case scenarios — operations at document extremes.

// ─────────────────────────────────────────────────────────────────────────────
// Operations at Document Start
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(boundary_start_h, "hello", "h");
neovim_test!(boundary_start_k, "hello\nworld", "k");
neovim_test!(boundary_start_0, "hello", "0");
neovim_test!(boundary_start_gg, "hello\nworld", "gg");
neovim_test!(boundary_start_dgg, "hello\nworld\ntest", "dgg");
neovim_test!(boundary_start_P, "hello\nworld", "ywP");

// ─────────────────────────────────────────────────────────────────────────────
// Operations at Document End
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(boundary_end_l, "hello", cursor(0, 4), "l");
neovim_test!(boundary_end_j, "hello\nworld", cursor(1, 0), "j");
neovim_test!(boundary_end_dollar, "hello", cursor(0, 4), "$");
neovim_test!(boundary_end_G, "hello\nworld", cursor(1, 0), "G");
neovim_test!(boundary_end_dG, "hello\nworld\ntest", cursor(2, 0), "dG");
neovim_test!(boundary_end_p, "hello\nworld", cursor(1, 0), "ywp");
neovim_test!(boundary_end_J, "hello\nworld", cursor(1, 0), "J");
neovim_test!(boundary_end_o, "hello", cursor(0, 4), "onew<Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Operations on Empty Lines
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(boundary_empty_dd, "aaa\n\nccc", cursor(1, 0), "dd");
neovim_test!(boundary_empty_cc, "aaa\n\nccc", cursor(1, 0), "ccnew<Esc>");
neovim_test!(boundary_empty_yy, "aaa\n\nccc", cursor(1, 0), "yyp");
neovim_test!(boundary_empty_dw, "aaa\n\nccc", cursor(1, 0), "dw");
neovim_test!(boundary_empty_x, "aaa\n\nccc", cursor(1, 0), "x");
neovim_test!(boundary_empty_indent, "aaa\n\nccc", cursor(1, 0), ">>");
neovim_test!(boundary_empty_J, "aaa\n\nccc", cursor(1, 0), "J");
neovim_test!(boundary_empty_o, "\n", "onew<Esc>");
neovim_test!(boundary_empty_O, "\n", "Onew<Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Operations on Single-Character Buffer
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(boundary_single_dd, "a", "dd");
neovim_test!(boundary_single_x, "a", "x");
neovim_test!(boundary_single_cw, "a", "cwb<Esc>");
neovim_test!(boundary_single_dw, "a", "dw");
neovim_test!(boundary_single_diw, "a", "diw");
neovim_test!(boundary_single_yy_p, "a", "yyp");
neovim_test!(boundary_single_J, "a", "J");

// ─────────────────────────────────────────────────────────────────────────────
// Search Boundary Cases
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(boundary_search_no_match, "hello world", "/zzzzz<CR>");
neovim_test!(boundary_search_wrap, "aaa bbb aaa", cursor(0, 5), "/aaa<CR>");
neovim_test!(boundary_star_single_match, "unique word here", "*");
neovim_test!(boundary_n_at_last_match, "foo bar foo", "/foo<CR>n");
neovim_test!(boundary_N_at_first_match, "foo bar foo", "/foo<CR>N");

// ─────────────────────────────────────────────────────────────────────────────
// Whitespace-Only Lines
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(boundary_ws_dw, "   ", "dw");
neovim_test!(boundary_ws_diw, "   ", "diw");
neovim_test!(boundary_ws_cw, "   ", "cwX<Esc>");
neovim_test!(boundary_ws_dd, "   ", "dd");
neovim_test!(boundary_ws_dollar, "   ", "$");
neovim_test!(boundary_ws_caret, "   ", "^");
neovim_test!(boundary_ws_w, "   ", "w");
