// Scenario fidelity tests: Join and Split Operations
//
// Line joining (J, gJ) and splitting patterns.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC JOIN
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, J_basic_two, "hello\nworld", "J");
neovim_test!(scenarios, J_three_lines, "a\nb\nc", "JJ");
neovim_test!(scenarios, J_count_3, "a\nb\nc\nd", "3J");
neovim_test!(scenarios, J_count_2, "hello\nworld", "2J");
neovim_test!(scenarios, J_preserves_indent, "hello\n    world", "J");
neovim_test!(scenarios, J_trailing_spaces, "hello   \nworld", "J");
neovim_test!(scenarios, J_empty_line, "hello\n\nworld", "J");
neovim_test!(scenarios, J_at_last_line, "only", "J");
neovim_test!(scenarios, J_second_to_last, "first\nlast", cursor(0, 0), "J");

// ═══════════════════════════════════════════════════════════════════════════════
// gJ (JOIN WITHOUT SPACE)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, gJ_basic, "hello\nworld", "gJ");
neovim_test!(scenarios, gJ_no_space, "a\nb", "gJ");
neovim_test!(scenarios, gJ_count_3, "a\nb\nc\nd", "3gJ");
neovim_test!(scenarios, gJ_preserves_leading, "hello\n    world", "gJ");
neovim_test!(scenarios, gJ_count_2, "a\nb\nc", "2gJ");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL JOIN
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, V_join, "l1\nl2\nl3", "VjJ");
neovim_test!(scenarios, V_join_all, "l1\nl2\nl3\nl4", "VGJ");
neovim_test!(scenarios, V_gJ, "l1\nl2\nl3", "VjgJ");

// ═══════════════════════════════════════════════════════════════════════════════
// SPLIT (i<CR>)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, split_at_space, "hello world", cursor(0, 5), "i<CR><Esc>");
neovim_test!(scenarios, split_at_comma, "a, b, c", cursor(0, 2), "a<CR><Esc>");
neovim_test!(scenarios, split_at_brace, "fn foo() { body(); }", cursor(0, 10), "a<CR><Esc>");
neovim_test!(scenarios, split_mid_word, "together", cursor(0, 4), "i<CR><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// JOIN THEN EDIT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, join_and_fix_space, "hello  \n  world", "Jf  x");
neovim_test!(scenarios, join_add_comma, "item1\nitem2", "A,<Esc>J");
neovim_test!(scenarios, join_3_then_edit, "a\nb\nc\nd", "3Jf cwX<Esc>");
