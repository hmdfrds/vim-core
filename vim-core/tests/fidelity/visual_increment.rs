// Visual mode increment/decrement fidelity tests.
//
// Ctrl-A / Ctrl-X in visual mode, and g Ctrl-A / g Ctrl-X for sequential.

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL Ctrl-A — INCREMENT SELECTED NUMBERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual_increment, v_ctrl_a_single, "1", "v<C-a>");
neovim_test!(visual_increment, v_ctrl_a_word, "count = 5", "f5v<C-a>");
neovim_test!(visual_increment, v_ctrl_a_line, "num: 10", "V<C-a>");
neovim_test!(visual_increment, v_ctrl_a_multiple_lines, "1\n2\n3", "Vjj<C-a>");
neovim_test!(visual_increment, v_ctrl_a_with_count, "5", "v3<C-a>");
neovim_test!(visual_increment, v_ctrl_a_negative, "-1", "v<C-a>");
neovim_test!(visual_increment, v_ctrl_a_zero, "0", "v<C-a>");
neovim_test!(visual_increment, v_ctrl_a_mixed_text, "a 1 b 2 c 3", "V<C-a>");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL Ctrl-X — DECREMENT SELECTED NUMBERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual_increment, v_ctrl_x_single, "5", "v<C-x>");
neovim_test!(visual_increment, v_ctrl_x_line, "num: 10", "V<C-x>");
neovim_test!(visual_increment, v_ctrl_x_multiple_lines, "5\n10\n15", "Vjj<C-x>");
neovim_test!(visual_increment, v_ctrl_x_with_count, "10", "v5<C-x>");
neovim_test!(visual_increment, v_ctrl_x_to_negative, "0", "v<C-x>");
neovim_test!(visual_increment, v_ctrl_x_already_negative, "-5", "v<C-x>");

// ═══════════════════════════════════════════════════════════════════════════════
// g Ctrl-A — SEQUENTIAL INCREMENT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual_increment, g_ctrl_a_basic, "0\n0\n0", "Vjjg<C-a>");
neovim_test!(visual_increment, g_ctrl_a_from_one, "1\n1\n1", "Vjjg<C-a>");
neovim_test!(visual_increment, g_ctrl_a_with_count, "0\n0\n0", "Vjj2g<C-a>");
neovim_test!(visual_increment, g_ctrl_a_mixed, "0\n0\n0\n0\n0", "Vjjjjg<C-a>");
neovim_test!(visual_increment, g_ctrl_a_with_text, "item 0\nitem 0\nitem 0", "Vjjg<C-a>");

// ═══════════════════════════════════════════════════════════════════════════════
// g Ctrl-X — SEQUENTIAL DECREMENT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual_increment, g_ctrl_x_basic, "0\n0\n0", "Vjjg<C-x>");
neovim_test!(visual_increment, g_ctrl_x_from_five, "5\n5\n5", "Vjjg<C-x>");
neovim_test!(visual_increment, g_ctrl_x_with_count, "10\n10\n10", "Vjj2g<C-x>");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL BLOCK Ctrl-A / Ctrl-X
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual_increment, block_ctrl_a, "1 a\n2 b\n3 c", "<C-v>jj<C-a>");
neovim_test!(visual_increment, block_ctrl_x, "5 a\n6 b\n7 c", "<C-v>jj<C-x>");
neovim_test!(visual_increment, block_g_ctrl_a, "0 a\n0 b\n0 c", "<C-v>jjg<C-a>");
neovim_test!(visual_increment, block_g_ctrl_x, "5 a\n5 b\n5 c", "<C-v>jjg<C-x>");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual_increment, v_ctrl_a_no_number, "hello", "v$<C-a>");
neovim_test!(visual_increment, v_ctrl_a_hex, "0xff", "v$<C-a>");
neovim_test!(visual_increment, v_ctrl_a_octal, "0o7", "v$<C-a>");
neovim_test!(visual_increment, v_ctrl_a_binary, "0b11", "v$<C-a>");
neovim_test!(visual_increment, v_ctrl_a_large_number, "999", "v<C-a>");
neovim_test!(visual_increment, v_ctrl_x_at_zero_boundary, "1", "v<C-x>");
