// Operator Cancel fidelity tests.
//
// Tests for cancelling operators mid-way with Escape, Ctrl-C, Ctrl-[,
// and verifying the buffer remains unchanged.

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL WITH ESCAPE
// ═══════════════════════════════════════════════════════════════════════════════

// Pending operator cancelled by Escape
neovim_test!(scenarios, cancel_d_esc, "hello world", "d<Esc>");
neovim_test!(scenarios, cancel_c_esc, "hello world", "c<Esc>");
neovim_test!(scenarios, cancel_y_esc, "hello world", "y<Esc>");
neovim_test!(scenarios, cancel_gu_esc, "HELLO", "gu<Esc>");
neovim_test!(scenarios, cancel_gU_esc, "hello", "gU<Esc>");
neovim_test!(scenarios, cancel_g_tilde_esc, "hello", "g~<Esc>");
neovim_test!(scenarios, cancel_gq_esc, "hello", "gq<Esc>");
neovim_test!(scenarios, cancel_indent_esc, "hello", "><Esc>");
neovim_test!(scenarios, cancel_outdent_esc, "    hello", "<<Esc>");
neovim_test!(scenarios, cancel_equal_esc, "hello", "=<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL WITH Ctrl-C
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cancel_d_ctrl_c, "hello world", "d<C-c>");
neovim_test!(scenarios, cancel_c_ctrl_c, "hello world", "c<C-c>");
neovim_test!(scenarios, cancel_y_ctrl_c, "hello world", "y<C-c>");
neovim_test!(scenarios, cancel_gu_ctrl_c, "HELLO", "gu<C-c>");
neovim_test!(scenarios, cancel_gU_ctrl_c, "hello", "gU<C-c>");
neovim_test!(scenarios, cancel_g_tilde_ctrl_c, "hello", "g~<C-c>");
neovim_test!(scenarios, cancel_indent_ctrl_c, "hello", "><C-c>");
neovim_test!(scenarios, cancel_outdent_ctrl_c, "    hello", "<<C-c>");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL WITH Ctrl-[
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cancel_d_ctrl_bracket, "hello world", "d<C-[>");
neovim_test!(scenarios, cancel_c_ctrl_bracket, "hello world", "c<C-[>");
neovim_test!(scenarios, cancel_y_ctrl_bracket, "hello world", "y<C-[>");
neovim_test!(scenarios, cancel_gu_ctrl_bracket, "HELLO", "gu<C-[>");
neovim_test!(scenarios, cancel_gU_ctrl_bracket, "hello", "gU<C-[>");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL THEN RETRY
// ═══════════════════════════════════════════════════════════════════════════════

// Cancel d then actually delete
neovim_test!(scenarios, cancel_d_then_dw, "hello world", "d<Esc>dw");
neovim_test!(scenarios, cancel_c_then_cw, "hello world", "c<Esc>cwX<Esc>");
neovim_test!(scenarios, cancel_y_then_yw, "hello world", "y<Esc>yw$p");
neovim_test!(scenarios, cancel_gu_then_guw, "HELLO WORLD", "gu<Esc>guw");
neovim_test!(scenarios, cancel_gU_then_gUw, "hello world", "gU<Esc>gUw");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL f/t IN OPERATOR PENDING
// ═══════════════════════════════════════════════════════════════════════════════

// df then Escape before finding char
neovim_test!(scenarios, cancel_df_esc, "hello world", "df<Esc>");
neovim_test!(scenarios, cancel_dt_esc, "hello world", "dt<Esc>");
neovim_test!(scenarios, cancel_cf_esc, "hello world", "cf<Esc>");
neovim_test!(scenarios, cancel_ct_esc, "hello world", "ct<Esc>");
neovim_test!(scenarios, cancel_yf_esc, "hello world", "yf<Esc>");
neovim_test!(scenarios, cancel_yt_esc, "hello world", "yt<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL g PREFIX
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cancel_g_esc, "hello", "g<Esc>");
neovim_test!(scenarios, cancel_g_ctrl_c, "hello", "g<C-c>");
neovim_test!(scenarios, cancel_g_then_guw, "HELLO", "g<Esc>guw");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL z PREFIX
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cancel_z_esc, "hello", "z<Esc>");
neovim_test!(scenarios, cancel_z_ctrl_c, "hello", "z<C-c>");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL [ and ] PREFIX
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cancel_open_bracket_esc, "hello", "[<Esc>");
neovim_test!(scenarios, cancel_close_bracket_esc, "hello", "]<Esc>");
neovim_test!(scenarios, cancel_open_bracket_ctrl_c, "hello", "[<C-c>");
neovim_test!(scenarios, cancel_close_bracket_ctrl_c, "hello", "]<C-c>");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL r (REPLACE CHAR)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cancel_r_esc, "hello", "r<Esc>");
neovim_test!(scenarios, cancel_r_ctrl_c, "hello", "r<C-c>");
neovim_test!(scenarios, cancel_r_then_r, "hello", "r<Esc>rx");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL q (MACRO RECORDING START)
// ═══════════════════════════════════════════════════════════════════════════════

// q awaits register — cancel before providing register
neovim_test!(scenarios, cancel_q_esc, "hello", "q<Esc>");
neovim_test!(scenarios, cancel_q_ctrl_c, "hello", "q<C-c>");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL @ (MACRO PLAYBACK)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cancel_at_esc, "hello", "@<Esc>");
neovim_test!(scenarios, cancel_at_ctrl_c, "hello", "@<C-c>");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL " (REGISTER PREFIX)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cancel_register_esc, "hello", "\"<Esc>");
neovim_test!(scenarios, cancel_register_ctrl_c, "hello", "\"<C-c>");
neovim_test!(scenarios, cancel_register_a_esc, "hello", "\"a<Esc>");
neovim_test!(scenarios, cancel_register_a_d_esc, "hello", "\"ad<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL INSERT MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cancel_insert_esc, "hello", "i<Esc>");
neovim_test!(scenarios, cancel_insert_ctrl_c, "hello", "i<C-c>");
neovim_test!(scenarios, cancel_insert_ctrl_bracket, "hello", "i<C-[>");
neovim_test!(scenarios, cancel_append_esc, "hello", "a<Esc>");
neovim_test!(scenarios, cancel_open_below_esc, "hello", "o<Esc>");
neovim_test!(scenarios, cancel_open_above_esc, "hello", "O<Esc>");
neovim_test!(scenarios, cancel_change_word_esc, "hello world", "cw<Esc>");
neovim_test!(scenarios, cancel_change_line_esc, "hello", "cc<Esc>");
neovim_test!(scenarios, cancel_substitute_esc, "hello", "s<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL VISUAL MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cancel_v_esc, "hello world", "vw<Esc>");
neovim_test!(scenarios, cancel_v_ctrl_c, "hello world", "vw<C-c>");
neovim_test!(scenarios, cancel_V_esc, "hello\nworld", "Vj<Esc>");
neovim_test!(scenarios, cancel_V_ctrl_c, "hello\nworld", "Vj<C-c>");
neovim_test!(scenarios, cancel_ctrl_v_esc, "hello\nworld", "<C-v>j<Esc>");
neovim_test!(scenarios, cancel_ctrl_v_ctrl_c, "hello\nworld", "<C-v>j<C-c>");

// Cancel then do something else
neovim_test!(scenarios, cancel_v_then_delete, "hello world", "vw<Esc>dw");
neovim_test!(scenarios, cancel_V_then_delete, "hello\nworld", "Vj<Esc>dd");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL SEARCH
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cancel_search_fwd_esc, "hello world", "/test<Esc>");
neovim_test!(scenarios, cancel_search_bwd_esc, "hello world", "?test<Esc>");
neovim_test!(scenarios, cancel_search_fwd_ctrl_c, "hello world", "/test<C-c>");
neovim_test!(scenarios, cancel_search_then_search, "hello world hello", "/wrong<Esc>/hello<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL REPLACE MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cancel_R_esc, "hello", "R<Esc>");
neovim_test!(scenarios, cancel_R_ctrl_c, "hello", "R<C-c>");
neovim_test!(scenarios, cancel_R_after_typing, "hello", "RXY<Esc>");
neovim_test!(scenarios, cancel_R_ctrl_bracket, "hello", "R<C-[>");

// ═══════════════════════════════════════════════════════════════════════════════
// DOUBLE ESCAPE (ALREADY IN NORMAL MODE)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, double_esc_normal, "hello", "<Esc><Esc>");
neovim_test!(scenarios, triple_esc_normal, "hello", "<Esc><Esc><Esc>");
neovim_test!(scenarios, esc_then_motion, "hello world", "<Esc>w");
neovim_test!(scenarios, esc_then_delete, "hello world", "<Esc>dw");

// ═══════════════════════════════════════════════════════════════════════════════
// CANCEL WITH COUNT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cancel_count_d_esc, "hello world", "3d<Esc>");
neovim_test!(scenarios, cancel_count_c_esc, "hello world", "3c<Esc>");
neovim_test!(scenarios, cancel_count_y_esc, "hello world", "3y<Esc>");
neovim_test!(scenarios, cancel_count_esc, "hello world", "3<Esc>");
neovim_test!(scenarios, cancel_count_then_op, "hello world", "3<Esc>dw");
