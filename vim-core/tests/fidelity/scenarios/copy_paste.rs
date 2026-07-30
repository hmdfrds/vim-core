// Scenario fidelity tests: Copy/Paste Workflows
//
// Multi-step workflows for yank and put patterns.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC YANK AND PASTE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, yank_word_paste, "hello world", "yw$p");
neovim_test!(scenarios, yank_line_paste_below, "original", "yyp");
neovim_test!(scenarios, yank_line_paste_above, "original", "yyP");
neovim_test!(scenarios, yank_inner_word_paste, "hello world", "yiwAP<Esc>");
neovim_test!(scenarios, yank_to_eol_paste, "hello world", cursor(0, 6), "y$$p");

// ═══════════════════════════════════════════════════════════════════════════════
// DELETE AND PASTE (MOVE TEXT)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dd_paste_below, "first\nsecond", "ddp");
neovim_test!(scenarios, dd_paste_above, "first\nsecond", cursor(1, 0), "ddP");
neovim_test!(scenarios, dw_paste_end, "moved rest", "dw$p");
neovim_test!(scenarios, delete_word_paste_elsewhere, "hello world test", "daw$p");
neovim_test!(scenarios, swap_lines_dd, "alpha\nbeta", "ddp");

// ═══════════════════════════════════════════════════════════════════════════════
// NAMED REGISTERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, named_reg_yank_paste, "hello world", "\"ayw$\"ap");
neovim_test!(scenarios, named_reg_two_regs, "aaa bbb", "\"ayww\"bywG\"ap\"bp");
neovim_test!(scenarios, named_reg_line, "content", "\"ayy\"ap");
neovim_test!(scenarios, blackhole_delete_word, "keep delete", "\"_daw");

// ═══════════════════════════════════════════════════════════════════════════════
// DUPLICATE PATTERNS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, duplicate_line_3x, "template", "yy3p");
neovim_test!(scenarios, duplicate_and_edit, "fn old() {}", "yypwcwnew<Esc>");
neovim_test!(scenarios, duplicate_line_then_modify, "let x = 1;", "yypwciwb<Esc>$ciw2<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL PASTE (SWAP)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, visual_paste_swap, "aaa bbb", "yiwwviwp");
neovim_test!(scenarios, visual_paste_line, "l1\nl2", "yyjVp");
neovim_test!(scenarios, visual_paste_inner, "text (old) rest", cursor(0, 6), "yi(fovi(p");

// ═══════════════════════════════════════════════════════════════════════════════
// MULTI-REGISTER PASTE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, two_reg_combine, "alpha beta", "\"ayww\"byw$\"bp\"ap");
neovim_test!(scenarios, zero_reg_after_delete, "keep delete", "ywdw\"0p");
neovim_test!(scenarios, cp_reg_overwrite, "first second", "\"ayww\"ayw$\"ap");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNT PASTE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, paste_3x_word, "word ", "yw3p");
neovim_test!(scenarios, paste_5x_char, "X", "yl5p");
neovim_test!(scenarios, paste_2x_line, "line", "yy2p");

// ═══════════════════════════════════════════════════════════════════════════════
// CROSS-LINE OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, yank_line_paste_3_down, "src\na\nb\nc", "yy3jp");
neovim_test!(scenarios, yank_word_paste_below, "source\ntarget", "ywjwp");
neovim_test!(scenarios, delete_line_paste_end, "move\nkeep1\nkeep2", "ddGp");
neovim_test!(scenarios, visual_yank_paste_far, "copy me\nmore\nmore\nmore\npaste here", "viwyjjjjp");

