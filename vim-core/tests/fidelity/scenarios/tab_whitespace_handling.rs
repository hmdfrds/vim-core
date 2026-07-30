// Tab and Whitespace Handling fidelity tests.
//
// Tests for how Vim handles tabs, mixed whitespace, and whitespace-only content
// across motions, operators, and text objects.

// ═══════════════════════════════════════════════════════════════════════════════
// MOTIONS ON TABS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, tab_w_over_tab, "hello\tworld", "w");
neovim_test!(scenarios, tab_b_over_tab, "hello\tworld", cursor(0, 6), "b");
neovim_test!(scenarios, tab_e_over_tab, "hello\tworld", "e");
neovim_test!(scenarios, tab_W_over_tab, "hello\tworld", "W");
neovim_test!(scenarios, tab_B_over_tab, "hello\tworld", cursor(0, 6), "B");
neovim_test!(scenarios, tab_E_over_tab, "hello\tworld", "E");
neovim_test!(scenarios, tab_0_on_tabbed, "\thello", cursor(0, 3), "0");
neovim_test!(scenarios, tab_caret_on_tabbed, "\thello", cursor(0, 0), "^");
neovim_test!(scenarios, tab_dollar_on_tabbed, "\thello", "$");
neovim_test!(scenarios, tab_g_underscore_tabbed, "\thello\t", "g_");
neovim_test!(scenarios, tab_f_tab, "hello\tworld", "f\t");
neovim_test!(scenarios, tab_t_tab, "hello\tworld", "t\t");
neovim_test!(scenarios, tab_F_tab, "hello\tworld", cursor(0, 6), "F\t");
neovim_test!(scenarios, tab_T_tab, "hello\tworld", cursor(0, 6), "T\t");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATORS ON TABS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, tab_dw_at_tab, "\thello", "dw");
neovim_test!(scenarios, tab_de_at_tab, "\thello", "de");
neovim_test!(scenarios, tab_cw_at_tab, "\thello", "cwX<Esc>");
neovim_test!(scenarios, tab_yw_at_tab, "\thello", "yw$p");
neovim_test!(scenarios, tab_x_on_tab, "\thello", "x");
neovim_test!(scenarios, tab_dd_tabbed_line, "\thello", "dd");
neovim_test!(scenarios, tab_D_from_tab, "\thello", "D");
neovim_test!(scenarios, tab_cc_tabbed, "\thello", "ccX<Esc>");
neovim_test!(scenarios, tab_C_from_tab, "\thello\tworld", "CX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// TAB INDENTATION
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, tab_indent_tabbed, "\thello", ">>");
neovim_test!(scenarios, tab_outdent_tabbed, "\thello", "<<");
neovim_test!(scenarios, tab_double_indent, "\thello", ">>.");
neovim_test!(scenarios, tab_indent_then_outdent, "hello", ">><< ");
neovim_test!(scenarios, tab_indent_multiple, "\thello\n\tworld", ">j");
neovim_test!(scenarios, tab_outdent_multiple, "\t\thello\n\t\tworld", "<j");

// ═══════════════════════════════════════════════════════════════════════════════
// MIXED TABS AND SPACES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, mixed_ws_w, "  \t  hello", "w");
neovim_test!(scenarios, mixed_ws_b, "  \t  hello", cursor(0, 7), "b");
neovim_test!(scenarios, mixed_ws_caret, "  \t  hello", cursor(0, 7), "^");
neovim_test!(scenarios, mixed_ws_dw, "  \t  hello", "dw");
neovim_test!(scenarios, mixed_ws_diw, "  \t  hello", "diw");
neovim_test!(scenarios, mixed_ws_daw, "  \t  hello", "daw");

// Multiple tabs
neovim_test!(scenarios, multi_tab_w, "\t\thello", "w");
neovim_test!(scenarios, multi_tab_caret, "\t\thello", "^");
neovim_test!(scenarios, multi_tab_0, "\t\thello", cursor(0, 3), "0");
neovim_test!(scenarios, multi_tab_dw, "\t\thello", "dw");
neovim_test!(scenarios, multi_tab_cc, "\t\thello", "ccX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// WHITESPACE-ONLY BUFFERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ws_only_spaces_w, "     ", "w");
neovim_test!(scenarios, ws_only_spaces_b, "     ", cursor(0, 4), "b");
neovim_test!(scenarios, ws_only_spaces_e, "     ", "e");
neovim_test!(scenarios, ws_only_spaces_dw, "     ", "dw");
neovim_test!(scenarios, ws_only_spaces_dd, "     ", "dd");
neovim_test!(scenarios, ws_only_spaces_diw, "     ", "diw");
neovim_test!(scenarios, ws_only_spaces_caret, "     ", "^");
neovim_test!(scenarios, ws_only_spaces_g_underscore, "     ", "g_");
neovim_test!(scenarios, ws_only_spaces_dollar, "     ", "$");
neovim_test!(scenarios, ws_only_spaces_cc, "     ", "ccX<Esc>");

neovim_test!(scenarios, ws_only_tabs_w, "\t\t\t", "w");
neovim_test!(scenarios, ws_only_tabs_b, "\t\t\t", cursor(0, 2), "b");
neovim_test!(scenarios, ws_only_tabs_dw, "\t\t\t", "dw");
neovim_test!(scenarios, ws_only_tabs_dd, "\t\t\t", "dd");
neovim_test!(scenarios, ws_only_tabs_diw, "\t\t\t", "diw");
neovim_test!(scenarios, ws_only_tabs_caret, "\t\t\t", "^");

// ═══════════════════════════════════════════════════════════════════════════════
// TABS IN MULTILINE CONTEXT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, tab_j_preserve_col, "\thello\n\tworld", "llj");
neovim_test!(scenarios, tab_k_preserve_col, "\thello\n\tworld", cursor(1, 3), "k");
neovim_test!(scenarios, tab_plus_indent, "hello\n\tworld", "+");
neovim_test!(scenarios, tab_minus_indent, "\thello\nworld", cursor(1, 0), "-");
neovim_test!(scenarios, tab_yy_p_tabbed, "\thello\n\tworld", "yyp");
neovim_test!(scenarios, tab_dd_tabbed_multi, "\thello\n\tworld\n\ttest", "dd");
neovim_test!(scenarios, tab_J_tabbed, "\thello\n\tworld", "J");
neovim_test!(scenarios, tab_gJ_tabbed, "\thello\n\tworld", "gJ");

// ═══════════════════════════════════════════════════════════════════════════════
// TABS WITH VISUAL MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, tab_v_select_through, "\thello\tworld", "v$d");
neovim_test!(scenarios, tab_V_tabbed, "\thello\n\tworld", "Vjd");
neovim_test!(scenarios, tab_ctrl_v_tabbed, "\thello\n\tworld", "<C-v>jd");
neovim_test!(scenarios, tab_viw_on_tab, "hello\tworld", cursor(0, 5), "viwd");
neovim_test!(scenarios, tab_vaw_on_tab, "hello\tworld", cursor(0, 5), "vawd");

// ═══════════════════════════════════════════════════════════════════════════════
// TABS WITH TEXT OBJECTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, tab_diw_on_tab_char, "hello\tworld", cursor(0, 5), "diw");
neovim_test!(scenarios, tab_daw_on_tab_char, "hello\tworld", cursor(0, 5), "daw");
neovim_test!(scenarios, tab_di_paren_with_tabs, "(\thello\t)", cursor(0, 3), "di(");
neovim_test!(scenarios, tab_di_brace_with_tabs, "{\thello\t}", cursor(0, 3), "di{");
neovim_test!(scenarios, tab_di_dquote_with_tabs, "\"\thello\t\"", cursor(0, 3), "di\"");

// ═══════════════════════════════════════════════════════════════════════════════
// TABS WITH SEARCH
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, tab_search_tab_char, "hello\tworld", "/\\t<CR>");
neovim_test!(scenarios, tab_search_word_after_tab, "hello\tworld", "/world<CR>");
neovim_test!(scenarios, tab_star_on_tabbed, "\thello", "w*");

// ═══════════════════════════════════════════════════════════════════════════════
// TABS WITH INSERT MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, tab_i_at_tab, "\thello", "iX<Esc>");
neovim_test!(scenarios, tab_a_at_tab, "\thello", "aX<Esc>");
neovim_test!(scenarios, tab_I_tabbed, "\thello", "IX<Esc>");
neovim_test!(scenarios, tab_A_tabbed, "\thello", "AX<Esc>");
neovim_test!(scenarios, tab_o_from_tabbed, "\thello", "oX<Esc>");
neovim_test!(scenarios, tab_O_from_tabbed, "\thello", "OX<Esc>");
neovim_test!(scenarios, tab_insert_tab_key, "hello", "i<Tab><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// TABS WITH DOT REPEAT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, tab_dot_dw, "\thello\tworld", "dw.");
neovim_test!(scenarios, tab_dot_cw, "\thello\tworld", "cwX<Esc>w.");
neovim_test!(scenarios, tab_dot_indent, "\thello\n\tworld", ">>j.");
neovim_test!(scenarios, tab_dot_x, "\thello", "x.");

// ═══════════════════════════════════════════════════════════════════════════════
// TABS WITH UNDO
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, tab_undo_dw, "\thello", "dwu");
neovim_test!(scenarios, tab_undo_dd, "\thello\n\tworld", "ddu");
neovim_test!(scenarios, tab_undo_indent, "hello", ">>u");
neovim_test!(scenarios, tab_undo_outdent, "\thello", "<<u");

// ═══════════════════════════════════════════════════════════════════════════════
// TRAILING WHITESPACE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, trailing_spaces_dollar, "hello   ", "$");
neovim_test!(scenarios, trailing_spaces_g_underscore, "hello   ", "g_");
neovim_test!(scenarios, trailing_spaces_dw, "hello   ", "dw");
neovim_test!(scenarios, trailing_spaces_D, "hello   ", cursor(0, 5), "D");
neovim_test!(scenarios, trailing_spaces_diw_end, "hello   ", cursor(0, 6), "diw");
neovim_test!(scenarios, trailing_tabs_dollar, "hello\t\t", "$");
neovim_test!(scenarios, trailing_tabs_g_underscore, "hello\t\t", "g_");
neovim_test!(scenarios, trailing_tabs_D, "hello\t\t", cursor(0, 5), "D");
neovim_test!(scenarios, trailing_mixed_dollar, "hello \t ", "$");
neovim_test!(scenarios, trailing_mixed_g_underscore, "hello \t ", "g_");

// ═══════════════════════════════════════════════════════════════════════════════
// LEADING WHITESPACE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, leading_spaces_caret, "    hello", "^");
neovim_test!(scenarios, leading_tabs_caret, "\t\thello", "^");
neovim_test!(scenarios, leading_mixed_caret, "  \t hello", "^");
neovim_test!(scenarios, leading_spaces_I, "    hello", "IX<Esc>");
neovim_test!(scenarios, leading_tabs_I, "\t\thello", "IX<Esc>");
neovim_test!(scenarios, leading_spaces_dw, "    hello", "dw");
neovim_test!(scenarios, leading_tabs_dw, "\t\thello", "dw");
neovim_test!(scenarios, leading_spaces_d_caret, "    hello", cursor(0, 6), "d^");
neovim_test!(scenarios, leading_tabs_d_caret, "\t\thello", cursor(0, 4), "d^");
