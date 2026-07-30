// Scenario fidelity tests: Count-Prefixed Workflows
//
// Real-world patterns using counts with motions, operators, and commands.

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTED MOTIONS IN EDITING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_3w_cw, "one two three four five", "3wcwnew<Esc>");
neovim_test!(scenarios, count_2j_dd, "l1\nl2\nl3\nl4", "2jdd");
neovim_test!(scenarios, count_5l_x, "abcdefghij", "5lx");
neovim_test!(scenarios, count_3b_dw, "a b c d e f", cursor(0, 10), "3bdw");
neovim_test!(scenarios, count_4j_cc, "l1\nl2\nl3\nl4\nl5\nl6", "4jccnew<Esc>");
neovim_test!(scenarios, count_2e_a, "one two three", "2ea!<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTED OPERATORS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_3dw, "one two three four five", "3dw");
neovim_test!(scenarios, count_2dd, "l1\nl2\nl3\nl4", "2dd");
neovim_test!(scenarios, count_3yy, "l1\nl2\nl3\nl4", "3yyGp");
neovim_test!(scenarios, count_2x, "abcde", "2x");
neovim_test!(scenarios, count_5x, "abcdefghij", "5x");
neovim_test!(scenarios, count_3J, "l1\nl2\nl3\nl4", "3J");
neovim_test!(scenarios, count_2cc, "l1\nl2\nl3\nl4", "2ccnew<Esc>");
neovim_test!(scenarios, count_3dj, "l1\nl2\nl3\nl4\nl5", "3dj");
neovim_test!(scenarios, count_2cw, "one two three", "2cwnew<Esc>");
neovim_test!(scenarios, count_4r, "hello", "4rx");
neovim_test!(scenarios, count_3s, "hello world", "3sX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTED DOT REPEAT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_dot_override, "abcdefghij", "2x3.");
neovim_test!(scenarios, count_dot_preserve, "abcdefghij", "3x.");
neovim_test!(scenarios, count_cw_dot, "old old old old", "cwnew<Esc>w.w.");
neovim_test!(scenarios, count_dd_dot, "l1\nl2\nl3\nl4\nl5\nl6", "2dd.");
neovim_test!(scenarios, count_dw_dot_3, "a b c d e f g h", "dw...");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTED FIND/TILL
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_3fo, "ooo ooo ooo", "3fo");
neovim_test!(scenarios, count_2fa, "abcabc", "2fa");
neovim_test!(scenarios, count_2ta, "abcabc", "2ta");
neovim_test!(scenarios, count_df_2, "a.b.c.d", "2df.");
neovim_test!(scenarios, count_ct_2, "a=b=c=d", "2ct=X<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTED INSERT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_3i, "hello", "3iX<Esc>");
neovim_test!(scenarios, count_5a, "hello", "5a!<Esc>");
neovim_test!(scenarios, count_3o, "hello", "3o<Esc>");
neovim_test!(scenarios, count_2O, "hello", "2O<Esc>");
neovim_test!(scenarios, count_10i_star, "", "10i*<Esc>");
neovim_test!(scenarios, count_3i_word, "", "3iha <Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTED PASTE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, count_3p, "hello world", "yw3p");
neovim_test!(scenarios, count_3P, "hello world", "yiw$3P");
neovim_test!(scenarios, count_2p_line, "original", "yy2p");
