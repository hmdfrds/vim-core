// Scenario fidelity tests: Case Operations
//
// Real-world case changing patterns in editing.

// ═══════════════════════════════════════════════════════════════════════════════
// TILDE (~) TOGGLE CASE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, tilde_single, "hello", "~");
neovim_test!(scenarios, tilde_whole_word, "hello", "~~~~~");
neovim_test!(scenarios, tilde_mixed, "hElLo", "~~~~~");
neovim_test!(scenarios, tilde_number_skip, "abc123def", "~~~~~~~~~");
neovim_test!(scenarios, tilde_3_count, "hello", "3~");

// ═══════════════════════════════════════════════════════════════════════════════
// gU (UPPERCASE) WORKFLOWS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, case_gU_word, "hello world", "gUw");
neovim_test!(scenarios, gU_inner_word, "hello world", cursor(0, 2), "gUiw");
neovim_test!(scenarios, gU_to_end, "hello world", "gU$");
neovim_test!(scenarios, gU_line, "hello world", "gUU");
neovim_test!(scenarios, gU_2w, "one two three", "gU2w");
neovim_test!(scenarios, gU_find, "hello.world", "gUf.");
neovim_test!(scenarios, gU_inner_parens, "(lower)", cursor(0, 1), "gUi(");
neovim_test!(scenarios, gU_inner_quotes, "\"lower\"", cursor(0, 1), "gUi\"");
neovim_test!(scenarios, case_gU_visual, "hello world", "vwgU");
neovim_test!(scenarios, case_gU_visual_line, "hello world", "VgU");

// ═══════════════════════════════════════════════════════════════════════════════
// gu (LOWERCASE) WORKFLOWS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, case_gu_word, "HELLO WORLD", "guw");
neovim_test!(scenarios, gu_inner_word, "HELLO WORLD", cursor(0, 2), "guiw");
neovim_test!(scenarios, gu_line, "HELLO WORLD", "guu");
neovim_test!(scenarios, gu_to_end, "HELLO WORLD", "gu$");
neovim_test!(scenarios, case_gu_visual, "HELLO WORLD", "vwgu");
neovim_test!(scenarios, gu_inner_quotes, "\"UPPER\"", cursor(0, 1), "gui\"");

// ═══════════════════════════════════════════════════════════════════════════════
// g~ (TOGGLE CASE) OPERATOR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, case_g_tilde_word, "HeLLo wORLd", "g~w");
neovim_test!(scenarios, g_tilde_inner_word, "HeLLo", cursor(0, 2), "g~iw");
neovim_test!(scenarios, case_g_tilde_line, "HeLLo WoRLd", "g~~");
neovim_test!(scenarios, g_tilde_visual, "HeLLo", "vwg~");

// ═══════════════════════════════════════════════════════════════════════════════
// CASE IN REALISTIC WORKFLOWS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, const_to_upper, "let max_size = 100;", cursor(0, 4), "gUiwIconst <Esc>");
neovim_test!(scenarios, uppercase_first_letter, "hello", "~");
neovim_test!(scenarios, make_constant, "maxRetries", "gUU");
neovim_test!(scenarios, fix_caps_word, "hELLO", "viwgu~");
neovim_test!(scenarios, screaming_snake, "hello_world", "gUU");
