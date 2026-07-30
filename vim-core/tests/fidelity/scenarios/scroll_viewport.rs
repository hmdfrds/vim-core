// Scroll and Viewport fidelity tests.
//
// Tests for all scroll commands with various buffer sizes, cursor positions,
// and edge cases.

// ═══════════════════════════════════════════════════════════════════════════════
// Ctrl-D — SCROLL DOWN HALF PAGE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ctrl_d_small_buffer, "l1\nl2\nl3", "<C-d>");
neovim_test!(scenarios, ctrl_d_medium_buffer, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "<C-d>");
neovim_test!(scenarios, ctrl_d_at_bottom, "l1\nl2\nl3\nl4\nl5", cursor(4, 0), "<C-d>");
neovim_test!(scenarios, ctrl_d_near_bottom, "l1\nl2\nl3\nl4\nl5", cursor(3, 0), "<C-d>");
neovim_test!(scenarios, ctrl_d_double, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "<C-d><C-d>");
neovim_test!(scenarios, ctrl_d_triple, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15", "<C-d><C-d><C-d>");
neovim_test!(scenarios, vp_ctrl_d_with_count, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "5<C-d>");
neovim_test!(scenarios, ctrl_d_count_remembered, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15", "3<C-d><C-d>");
neovim_test!(scenarios, vp_ctrl_d_single_line, "hello", "<C-d>");
neovim_test!(scenarios, ctrl_d_empty, "", "<C-d>");
neovim_test!(scenarios, ctrl_d_two_lines, "l1\nl2", "<C-d>");

// ═══════════════════════════════════════════════════════════════════════════════
// Ctrl-U — SCROLL UP HALF PAGE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ctrl_u_small_buffer, "l1\nl2\nl3", cursor(2, 0), "<C-u>");
neovim_test!(scenarios, ctrl_u_medium_buffer, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(9, 0), "<C-u>");
neovim_test!(scenarios, vp_ctrl_u_at_top, "l1\nl2\nl3\nl4\nl5", "<C-u>");
neovim_test!(scenarios, ctrl_u_near_top, "l1\nl2\nl3\nl4\nl5", cursor(1, 0), "<C-u>");
neovim_test!(scenarios, ctrl_u_double, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(9, 0), "<C-u><C-u>");
neovim_test!(scenarios, ctrl_u_with_count, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(9, 0), "5<C-u>");
neovim_test!(scenarios, vp_ctrl_u_single_line, "hello", "<C-u>");
neovim_test!(scenarios, ctrl_u_empty, "", "<C-u>");
neovim_test!(scenarios, ctrl_u_two_lines, "l1\nl2", cursor(1, 0), "<C-u>");

// ═══════════════════════════════════════════════════════════════════════════════
// Ctrl-F — SCROLL FORWARD FULL PAGE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ctrl_f_small_buffer, "l1\nl2\nl3", "<C-f>");
neovim_test!(scenarios, ctrl_f_medium_buffer, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "<C-f>");
neovim_test!(scenarios, ctrl_f_at_bottom, "l1\nl2\nl3", cursor(2, 0), "<C-f>");
neovim_test!(scenarios, ctrl_f_double, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nl16\nl17\nl18\nl19\nl20", "<C-f><C-f>");
neovim_test!(scenarios, ctrl_f_with_count, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nl16\nl17\nl18\nl19\nl20", "2<C-f>");
neovim_test!(scenarios, vp_ctrl_f_single_line, "hello", "<C-f>");
neovim_test!(scenarios, ctrl_f_empty, "", "<C-f>");

// ═══════════════════════════════════════════════════════════════════════════════
// Ctrl-B — SCROLL BACKWARD FULL PAGE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ctrl_b_small_buffer, "l1\nl2\nl3", cursor(2, 0), "<C-b>");
neovim_test!(scenarios, ctrl_b_medium_buffer, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(9, 0), "<C-b>");
neovim_test!(scenarios, ctrl_b_at_top, "l1\nl2\nl3", "<C-b>");
neovim_test!(scenarios, ctrl_b_double, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nl16\nl17\nl18\nl19\nl20", cursor(19, 0), "<C-b><C-b>");
neovim_test!(scenarios, ctrl_b_with_count, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12\nl13\nl14\nl15\nl16\nl17\nl18\nl19\nl20", cursor(19, 0), "2<C-b>");
neovim_test!(scenarios, vp_ctrl_b_single_line, "hello", "<C-b>");
neovim_test!(scenarios, ctrl_b_empty, "", "<C-b>");

// ═══════════════════════════════════════════════════════════════════════════════
// Ctrl-E — SCROLL DOWN ONE LINE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, vp_ctrl_e_basic, "l1\nl2\nl3\nl4\nl5", "<C-e>");
neovim_test!(scenarios, ctrl_e_multiple, "l1\nl2\nl3\nl4\nl5", "<C-e><C-e><C-e>");
neovim_test!(scenarios, ctrl_e_at_bottom, "l1\nl2\nl3", cursor(2, 0), "<C-e>");
neovim_test!(scenarios, ctrl_e_with_count, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "5<C-e>");
neovim_test!(scenarios, ctrl_e_single_line, "hello", "<C-e>");
neovim_test!(scenarios, ctrl_e_empty, "", "<C-e>");

// ═══════════════════════════════════════════════════════════════════════════════
// Ctrl-Y — SCROLL UP ONE LINE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, vp_ctrl_y_basic, "l1\nl2\nl3\nl4\nl5", cursor(4, 0), "<C-y>");
neovim_test!(scenarios, ctrl_y_multiple, "l1\nl2\nl3\nl4\nl5", cursor(4, 0), "<C-y><C-y><C-y>");
neovim_test!(scenarios, ctrl_y_at_top, "l1\nl2\nl3", "<C-y>");
neovim_test!(scenarios, ctrl_y_with_count, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(9, 0), "5<C-y>");
neovim_test!(scenarios, ctrl_y_single_line, "hello", "<C-y>");
neovim_test!(scenarios, ctrl_y_empty, "", "<C-y>");

// ═══════════════════════════════════════════════════════════════════════════════
// zz — CENTER CURSOR LINE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, zz_top, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "zz");
neovim_test!(scenarios, zz_middle, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(4, 0), "zz");
neovim_test!(scenarios, zz_bottom, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(9, 0), "zz");
neovim_test!(scenarios, zz_single, "hello", "zz");
neovim_test!(scenarios, zz_empty, "", "zz");
neovim_test!(scenarios, zz_two_lines, "l1\nl2", "zz");
neovim_test!(scenarios, zz_after_motion, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "5jzz");

// ═══════════════════════════════════════════════════════════════════════════════
// zt — CURSOR LINE TO TOP
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, vp_zt_top, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "zt");
neovim_test!(scenarios, zt_middle, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(4, 0), "zt");
neovim_test!(scenarios, zt_bottom, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(9, 0), "zt");
neovim_test!(scenarios, zt_single, "hello", "zt");
neovim_test!(scenarios, zt_empty, "", "zt");

// ═══════════════════════════════════════════════════════════════════════════════
// zb — CURSOR LINE TO BOTTOM
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, zb_top, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "zb");
neovim_test!(scenarios, zb_middle, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(4, 0), "zb");
neovim_test!(scenarios, vp_zb_bottom, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(9, 0), "zb");
neovim_test!(scenarios, zb_single, "hello", "zb");
neovim_test!(scenarios, zb_empty, "", "zb");

// ═══════════════════════════════════════════════════════════════════════════════
// z<CR> — TOP + FIRST NON-BLANK
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, z_cr_basic, "  hello\n  world\n  test", cursor(1, 4), "z\n");
neovim_test!(scenarios, vp_z_cr_no_indent, "hello\nworld", cursor(1, 0), "z\n");
neovim_test!(scenarios, z_cr_tabs, "\thello\n\tworld", cursor(1, 3), "z\n");
neovim_test!(scenarios, z_cr_empty_buffer, "", "z\n");

// ═══════════════════════════════════════════════════════════════════════════════
// z. — CENTER + FIRST NON-BLANK
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, z_dot_basic, "  hello\n  world\n  test", cursor(1, 4), "z.");
neovim_test!(scenarios, z_dot_no_indent, "hello\nworld", cursor(1, 0), "z.");
neovim_test!(scenarios, z_dot_tabs, "\thello\n\tworld", cursor(1, 3), "z.");
neovim_test!(scenarios, z_dot_empty_buffer, "", "z.");

// ═══════════════════════════════════════════════════════════════════════════════
// z- — BOTTOM + FIRST NON-BLANK
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, z_minus_basic, "  hello\n  world\n  test", cursor(1, 4), "z-");
neovim_test!(scenarios, z_minus_no_indent, "hello\nworld", cursor(1, 0), "z-");
neovim_test!(scenarios, z_minus_tabs, "\thello\n\tworld", cursor(1, 3), "z-");
neovim_test!(scenarios, z_minus_empty_buffer, "", "z-");

// ═══════════════════════════════════════════════════════════════════════════════
// H, M, L — SCREEN LINE MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, vp_H_basic, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(5, 0), "H");
neovim_test!(scenarios, vp_H_with_count, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(5, 0), "3H");
neovim_test!(scenarios, H_at_top, "l1\nl2\nl3\nl4\nl5", "H");
neovim_test!(scenarios, vp_H_single_line, "hello", "H");
neovim_test!(scenarios, H_empty, "", "H");
neovim_test!(scenarios, H_indented, "  l1\n  l2\n  l3\n  l4\n  l5\n  l6\n  l7\n  l8\n  l9\n  l10", cursor(5, 0), "H");

neovim_test!(scenarios, vp_M_basic, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(0, 0), "M");
neovim_test!(scenarios, M_at_middle, "l1\nl2\nl3\nl4\nl5", cursor(2, 0), "M");
neovim_test!(scenarios, vp_M_single_line, "hello", "M");
neovim_test!(scenarios, M_empty, "", "M");

neovim_test!(scenarios, vp_L_basic, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(0, 0), "L");
neovim_test!(scenarios, vp_L_with_count, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(0, 0), "3L");
neovim_test!(scenarios, L_at_bottom, "l1\nl2\nl3\nl4\nl5", cursor(4, 0), "L");
neovim_test!(scenarios, vp_L_single_line, "hello", "L");
neovim_test!(scenarios, L_empty, "", "L");

// ═══════════════════════════════════════════════════════════════════════════════
// SCROLL THEN OPERATOR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ctrl_d_then_dd, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "<C-d>dd");
neovim_test!(scenarios, ctrl_u_then_dd, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(9, 0), "<C-u>dd");
neovim_test!(scenarios, ctrl_f_then_x, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "<C-f>x");
neovim_test!(scenarios, zz_then_dd, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(4, 0), "zzdd");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATORS WITH H, M, L MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, vp_d_H, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(5, 0), "dH");
neovim_test!(scenarios, vp_d_M, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "dM");
neovim_test!(scenarios, vp_d_L, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "dL");
neovim_test!(scenarios, y_H, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(5, 0), "yHGp");
neovim_test!(scenarios, y_M, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "yMGp");
neovim_test!(scenarios, y_L, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "yLGp");
neovim_test!(scenarios, c_H, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(5, 0), "cHX<Esc>");
neovim_test!(scenarios, c_M, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "cMX<Esc>");
neovim_test!(scenarios, c_L, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "cLX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// INTERLEAVED SCROLL AND MOTION
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ctrl_d_ctrl_u, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "<C-d><C-u>");
neovim_test!(scenarios, ctrl_f_ctrl_b, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "<C-f><C-b>");
neovim_test!(scenarios, zz_zt_zb, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(4, 0), "zzztzbzz");
neovim_test!(scenarios, ctrl_e_ctrl_y, "l1\nl2\nl3\nl4\nl5", "<C-e><C-y>");
