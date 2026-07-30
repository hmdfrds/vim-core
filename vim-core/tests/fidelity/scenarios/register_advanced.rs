// Scenario fidelity tests: Advanced Register Workflows
//
// Numbered registers, append registers, Ctrl-R insert, visual put swap.

// ═══════════════════════════════════════════════════════════════════════════════
// NUMBERED REGISTERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, numbered_1_after_dd, "l1\nl2\nl3", "dd\"1p");
neovim_test!(scenarios, numbered_chain, "l1\nl2\nl3", "dddd\"1p\"2p");
neovim_test!(scenarios, numbered_after_dw, "one two three", "dwdw\"1p");
neovim_test!(scenarios, last_yank_0, "hello world", "ywdw\"0p");
neovim_test!(scenarios, yank_0_vs_unnamed, "keep delete", "yiw$daw0\"0P");

// ═══════════════════════════════════════════════════════════════════════════════
// APPEND TO REGISTERS (uppercase)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, append_reg, "l1\nl2\nl3", "\"ayyjj\"Ayy\"ap");
neovim_test!(scenarios, append_reg_words, "hello world foo", "\"ayww\"AywG\"ap");
neovim_test!(scenarios, append_reg_lines, "a\nb\nc", "\"ayyj\"Ayyj\"ap");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL PUT (SWAP)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, visual_put_swap, "aaa bbb", "yiwwviwp");
neovim_test!(scenarios, visual_put_line, "l1\nl2", "yyj Vp");
neovim_test!(scenarios, visual_put_twice, "aaa bbb ccc", "yiwwviwpwviwp");

// ═══════════════════════════════════════════════════════════════════════════════
// NAMED REGISTER WORKFLOWS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, reg_a_yank, "first second", "\"ayiww\"byw$\"ap\"bp");
neovim_test!(scenarios, reg_copy_two_words, "hello world", "\"ayww\"byw$\"bp\"ap");
neovim_test!(scenarios, reg_line_word, "full line\nword here", "\"ayyjw\"byw\"bp\"ap");
neovim_test!(scenarios, ra_reg_overwrite, "alpha beta", "\"ayww\"aywo<C-r>a<Esc>");
neovim_test!(scenarios, reg_5_named, "a b c d e", "\"ayww\"byww\"cyww\"dyww\"eyw");

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT MODE REGISTER (Ctrl-R)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, insert_ctrl_r_unnamed, "hello", "ywA <C-r>\"<Esc>");
neovim_test!(scenarios, insert_ctrl_r_named, "hello", "\"aywA <C-r>a<Esc>");
neovim_test!(scenarios, insert_ctrl_r_0, "hello world", "ywdwA<C-r>0<Esc>");
neovim_test!(scenarios, insert_ctrl_r_mid, "abc def", "ywwi<C-r>\"<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// BLACKHOLE REGISTER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, blackhole_dd, "l1\nl2\nl3", "yy\"_ddp");
neovim_test!(scenarios, blackhole_dw, "hello world", "yw\"_dwp");
neovim_test!(scenarios, ra_blackhole_x, "abc", "yw\"_xp");
neovim_test!(scenarios, blackhole_visual, "hello world", "yw\"_viwd0p");

// ═══════════════════════════════════════════════════════════════════════════════
// SMALL DELETE REGISTER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, small_delete_x_scenario, "abc", "x\"-p");
neovim_test!(scenarios, small_delete_dl, "abc", "dl\"-p");
neovim_test!(scenarios, small_delete_dw, "hello world", "dw\"-p");
