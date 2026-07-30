// Count Edge Cases fidelity tests.
//
// Tests for counts with every command type — verifying that counts work
// correctly, large counts clamp properly, count=0 behaves correctly,
// and multi-digit counts parse properly.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC MOTION COUNTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_1h, "hello world", cursor(0, 5), "1h");
neovim_test!(scenarios, count_5h, "hello world", cursor(0, 10), "5h");
neovim_test!(scenarios, count_10h, "hello world", cursor(0, 10), "10h");
neovim_test!(scenarios, count_100h, "hello world", cursor(0, 10), "100h");

neovim_test!(scenarios, count_1l, "hello world", "1l");
neovim_test!(scenarios, count_5l, "hello world", "5l");
neovim_test!(scenarios, cnt_count_10l, "hello world", "10l");
neovim_test!(scenarios, count_100l, "hello world", "100l");

neovim_test!(scenarios, count_1j, "l1\nl2\nl3\nl4\nl5", "1j");
neovim_test!(scenarios, count_3j, "l1\nl2\nl3\nl4\nl5", "3j");
neovim_test!(scenarios, count_100j, "l1\nl2\nl3", "100j");

neovim_test!(scenarios, count_1k, "l1\nl2\nl3\nl4\nl5", cursor(4, 0), "1k");
neovim_test!(scenarios, count_3k, "l1\nl2\nl3\nl4\nl5", cursor(4, 0), "3k");
neovim_test!(scenarios, count_100k, "l1\nl2\nl3", cursor(2, 0), "100k");

neovim_test!(scenarios, count_1w, "one two three four", "1w");
neovim_test!(scenarios, count_3w, "one two three four", "3w");
neovim_test!(scenarios, count_100w, "one two three", "100w");

neovim_test!(scenarios, count_1b, "one two three four", cursor(0, 14), "1b");
neovim_test!(scenarios, count_3b, "one two three four", cursor(0, 14), "3b");
neovim_test!(scenarios, count_100b, "one two three", cursor(0, 8), "100b");

neovim_test!(scenarios, count_1e, "one two three four", "1e");
neovim_test!(scenarios, count_3e, "one two three four", "3e");
neovim_test!(scenarios, count_100e, "one two three", "100e");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTS WITH OPERATORS
// ═══════════════════════════════════════════════════════════════════════════════

// Delete with count
neovim_test!(scenarios, count_1x, "abcdefgh", "1x");
neovim_test!(scenarios, count_3x, "abcdefgh", "3x");
neovim_test!(scenarios, cnt_count_5x, "abcdefgh", "5x");
neovim_test!(scenarios, count_100x, "abcdef", "100x");

neovim_test!(scenarios, count_1X, "abcdefgh", cursor(0, 5), "1X");
neovim_test!(scenarios, count_3X, "abcdefgh", cursor(0, 5), "3X");
neovim_test!(scenarios, count_100X, "abcdef", cursor(0, 3), "100X");

neovim_test!(scenarios, count_1dd, "l1\nl2\nl3\nl4", "1dd");
neovim_test!(scenarios, cnt_count_2dd, "l1\nl2\nl3\nl4", "2dd");
neovim_test!(scenarios, count_3dd, "l1\nl2\nl3\nl4", "3dd");
neovim_test!(scenarios, count_100dd, "l1\nl2\nl3", "100dd");

neovim_test!(scenarios, count_1dw, "one two three four", "1dw");
neovim_test!(scenarios, count_2dw, "one two three four", "2dw");
neovim_test!(scenarios, cnt_count_3dw, "one two three four", "3dw");
neovim_test!(scenarios, count_100dw, "one two three", "100dw");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNT × OPERATOR COUNT (MULTIPLY)
// ═══════════════════════════════════════════════════════════════════════════════

// 2d3w = d6w (count before operator × count before motion)
neovim_test!(scenarios, count_2d3w, "a b c d e f g h", "2d3w");
neovim_test!(scenarios, count_3d2w, "a b c d e f g h", "3d2w");
neovim_test!(scenarios, count_2d2l, "abcdefgh", "2d2l");
neovim_test!(scenarios, count_3d2l, "abcdefgh", "3d2l");

// Same for change
neovim_test!(scenarios, count_2c3w, "a b c d e f g h", "2c3wX<Esc>");
neovim_test!(scenarios, count_3c2w, "a b c d e f g h", "3c2wX<Esc>");

// Same for yank (verify with paste)
neovim_test!(scenarios, count_2y3w, "a b c d e f g h", "2y3w$p");
neovim_test!(scenarios, count_3y2w, "a b c d e f g h", "3y2w$p");

// Count doubled operators
neovim_test!(scenarios, count_2_indent, "l1\nl2\nl3\nl4", "2>>");
neovim_test!(scenarios, count_3_indent, "l1\nl2\nl3\nl4", "3>>");
neovim_test!(scenarios, count_2_outdent, "    l1\n    l2\n    l3\n    l4", "2<<");
neovim_test!(scenarios, count_2guu, "HELLO\nWORLD\nTEST", "2guu");
neovim_test!(scenarios, count_3gUU, "hello\nworld\ntest\nfour", "3gUU");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTS WITH INSERT MODE
// ═══════════════════════════════════════════════════════════════════════════════

// Count before insert commands
neovim_test!(scenarios, cnt_count_3i, "hello", "3iX<Esc>");
neovim_test!(scenarios, count_5i, "hello", "5iX<Esc>");
neovim_test!(scenarios, count_3a, "hello", "3aX<Esc>");
neovim_test!(scenarios, cnt_count_3o, "hello", "3oX<Esc>");
neovim_test!(scenarios, count_3O, "hello", "3OX<Esc>");
neovim_test!(scenarios, count_2I, "hello", "2IX<Esc>");
neovim_test!(scenarios, count_2A, "hello", "2AX<Esc>");
neovim_test!(scenarios, cnt_count_3s, "abcdefgh", "3sX<Esc>");
neovim_test!(scenarios, count_3S, "l1\nl2\nl3\nl4", "3SX<Esc>");

// Count with multi-char insert
neovim_test!(scenarios, count_3i_multichar, "hello", "3iAB<Esc>");
neovim_test!(scenarios, count_2a_multichar, "hello", "2aXY<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTS WITH CHANGE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cnt_count_2cc, "l1\nl2\nl3\nl4", "2ccX<Esc>");
neovim_test!(scenarios, count_3cc, "l1\nl2\nl3\nl4\nl5", "3ccX<Esc>");
neovim_test!(scenarios, count_100cc, "l1\nl2\nl3", "100ccX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTS WITH FIND MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cnt_count_2fa, "abcabc", "2fa");
neovim_test!(scenarios, count_3fa, "abcabcabc", "3fa");
neovim_test!(scenarios, count_100fa, "abcabc", "100fa");
neovim_test!(scenarios, cnt_count_2ta, "abcabc", "2ta");
neovim_test!(scenarios, count_2Fa, "abcabc", cursor(0, 5), "2Fa");
neovim_test!(scenarios, count_2Ta, "abcabc", cursor(0, 5), "2Ta");

// Counts with df/dt
neovim_test!(scenarios, count_d2fa, "abcabc", "d2fa");
neovim_test!(scenarios, count_d3fa, "abcabcabc", "d3fa");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTS WITH REPLACE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_1r, "hello", "1rX");
neovim_test!(scenarios, count_3r, "hello", "3rX");
neovim_test!(scenarios, count_5r, "hello", "5rX");
neovim_test!(scenarios, count_100r, "hello", "100rX");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTS WITH TILDE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_1_tilde, "hello", "1~");
neovim_test!(scenarios, count_3_tilde, "hello", "3~");
neovim_test!(scenarios, count_5_tilde, "hello", "5~");
neovim_test!(scenarios, count_100_tilde, "hello", "100~");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTS WITH SCROLL
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_3_ctrl_d, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "3<C-d>");
neovim_test!(scenarios, count_3_ctrl_u, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(9, 0), "3<C-u>");
neovim_test!(scenarios, count_2_ctrl_f, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "2<C-f>");
neovim_test!(scenarios, count_2_ctrl_b, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(9, 0), "2<C-b>");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTS WITH MACROS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_2_at, "abcdef", "qaxq2@a");
neovim_test!(scenarios, count_5_at, "abcdefgh", "qaxq5@a");
neovim_test!(scenarios, count_3_at_repeat, "abcdefgh", "qaxq@a3@@");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTS WITH DOT REPEAT
// ═══════════════════════════════════════════════════════════════════════════════

// Count before dot overrides original count
neovim_test!(scenarios, cnt_dot_count_override, "abcdefgh", "2x3.");
neovim_test!(scenarios, dot_count_preserve, "abcdefgh", "3x.");
neovim_test!(scenarios, cnt_dot_count_1, "abcdefgh", "3x1.");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTS WITH UNDO/REDO
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_2u, "abc", "xxx2u");
neovim_test!(scenarios, count_3u, "abcdef", "xxxxx3u");
neovim_test!(scenarios, count_2_ctrl_r, "abcdef", "xxxxuuuu2<C-r>");
neovim_test!(scenarios, count_3_ctrl_r, "abcdef", "xxxxuuuu3<C-r>");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTS WITH VISUAL MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_v_3l, "hello world", "v3ld");
neovim_test!(scenarios, count_v_2w, "one two three four", "v2wd");
neovim_test!(scenarios, count_V_2j, "l1\nl2\nl3\nl4", "V2jd");
neovim_test!(scenarios, count_ctrl_v_3j, "l1\nl2\nl3\nl4\nl5", "<C-v>3jd");

// ═══════════════════════════════════════════════════════════════════════════════
// MULTI-DIGIT COUNTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cnt_count_10l_multidigit, "a b c d e f g h i j k l m n o p", "10l");
neovim_test!(scenarios, count_15l, "a b c d e f g h i j k l m n o p q", "15l");
neovim_test!(scenarios, count_10j, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12", "10j");
neovim_test!(scenarios, count_10dd, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11\nl12", "10dd");
neovim_test!(scenarios, count_99x, "abcdefghijklmnopqrstuvwxyz", "99x");

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

// 0 is a motion (go to column 0), not a count
neovim_test!(scenarios, zero_is_motion, "hello world", cursor(0, 5), "0");
neovim_test!(scenarios, d0_is_delete_to_bol, "hello world", cursor(0, 5), "d0");
neovim_test!(scenarios, y0_is_yank_to_bol, "hello world", cursor(0, 5), "y0P");
neovim_test!(scenarios, c0_is_change_to_bol, "hello world", cursor(0, 5), "c0X<Esc>");

// 10 after operator: 1 is count, then 0 is motion start
neovim_test!(scenarios, count_10_G, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11", "10G");
neovim_test!(scenarios, count_10_gg, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10\nl11", cursor(10, 0), "10gg");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTS WITH J (JOIN)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_2J, "l1\nl2\nl3\nl4", "2J");
neovim_test!(scenarios, cnt_count_3J, "l1\nl2\nl3\nl4", "3J");
neovim_test!(scenarios, count_4J, "l1\nl2\nl3\nl4\nl5", "4J");
neovim_test!(scenarios, count_100J, "l1\nl2\nl3", "100J");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTS WITH PASTE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_2p, "hello", "yw2p");
neovim_test!(scenarios, cnt_count_3p, "hello", "yw3p");
neovim_test!(scenarios, count_5p, "X", "yl5p");
neovim_test!(scenarios, count_2P, "hello", "yw2P");
neovim_test!(scenarios, count_2p_linewise, "hello\nworld", "yy2p");
neovim_test!(scenarios, count_3p_linewise, "hello", "yy3p");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTS WITH $ (END OF LINE + COUNT DOWN)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_2_dollar, "l1\nl2\nl3\nl4", "2$");
neovim_test!(scenarios, count_3_dollar, "l1\nl2\nl3\nl4", "3$");
neovim_test!(scenarios, count_100_dollar, "l1\nl2\nl3", "100$");
