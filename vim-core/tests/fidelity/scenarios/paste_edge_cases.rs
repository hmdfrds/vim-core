// Paste Edge Cases fidelity tests.
//
// Comprehensive tests for p, P, gp, gP with linewise vs charwise registers,
// named registers, paste at boundaries, multiple pastes, and register interactions.

// ═══════════════════════════════════════════════════════════════════════════════
// CHARWISE PASTE (p/P)
// ═══════════════════════════════════════════════════════════════════════════════

// Basic charwise paste
neovim_test!(scenarios, paste_p_charwise, "hello world", "ywwp");
neovim_test!(scenarios, paste_P_charwise, "hello world", "ywwP");
neovim_test!(scenarios, paste_p_single_char, "hello", "ylp");
neovim_test!(scenarios, paste_P_single_char, "hello", "ylP");
neovim_test!(scenarios, paste_p_word_end, "hello", "yw$p");
neovim_test!(scenarios, paste_P_word_start, "hello world", cursor(0, 6), "ywP");

// Charwise paste at boundaries
neovim_test!(scenarios, paste_p_at_eol, "hello", "yw$p");
neovim_test!(scenarios, paste_P_at_bol, "hello", "ywP");
neovim_test!(scenarios, paste_p_empty_line, "hello\n\nworld", "ywjp");
neovim_test!(scenarios, paste_P_empty_line, "hello\n\nworld", "ywjP");

// Multiple charwise pastes
neovim_test!(scenarios, paste_p_multiple, "hello", "ywpp");
neovim_test!(scenarios, paste_p_three, "hello", "ywppp");
neovim_test!(scenarios, paste_P_multiple, "hello", "ywPP");

// Charwise paste with count
neovim_test!(scenarios, paste_p_count_2, "hello", "yw2p");
neovim_test!(scenarios, paste_p_count_3, "hello", "yw3p");
neovim_test!(scenarios, paste_P_count_2, "hello", "yw2P");
neovim_test!(scenarios, paste_p_count_5, "X", "yl5p");

// ═══════════════════════════════════════════════════════════════════════════════
// LINEWISE PASTE (p/P)
// ═══════════════════════════════════════════════════════════════════════════════

// Basic linewise paste
neovim_test!(scenarios, paste_p_linewise, "line1\nline2\nline3", "yyp");
neovim_test!(scenarios, paste_P_linewise, "line1\nline2\nline3", "yyP");
neovim_test!(scenarios, paste_p_dd_linewise, "line1\nline2\nline3", cursor(1, 0), "ddp");
neovim_test!(scenarios, paste_P_dd_linewise, "line1\nline2\nline3", cursor(1, 0), "ddP");

// Linewise paste at boundaries
neovim_test!(scenarios, paste_p_linewise_first, "line1\nline2", "yyp");
neovim_test!(scenarios, paste_P_linewise_first, "line1\nline2", "yyP");
neovim_test!(scenarios, paste_p_linewise_last, "line1\nline2", cursor(1, 0), "yyp");
neovim_test!(scenarios, paste_P_linewise_last, "line1\nline2", cursor(1, 0), "yyP");

// Linewise paste only line
neovim_test!(scenarios, paste_p_only_line, "hello", "yyp");
neovim_test!(scenarios, paste_P_only_line, "hello", "yyP");

// Multiple linewise pastes
neovim_test!(scenarios, paste_p_linewise_multi, "line1\nline2", "yyjpp");
neovim_test!(scenarios, paste_p_linewise_count, "line1\nline2", "yy2p");
neovim_test!(scenarios, paste_P_linewise_count, "line1\nline2", cursor(1, 0), "yy2P");

// ═══════════════════════════════════════════════════════════════════════════════
// PASTE FROM NAMED REGISTERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, paste_reg_a_charwise, "hello world", "\"ayw$\"ap");
neovim_test!(scenarios, paste_reg_a_linewise, "hello\nworld", "\"ayy$\"ap");
neovim_test!(scenarios, paste_reg_b_charwise, "hello world", "\"byw$\"bp");
neovim_test!(scenarios, paste_reg_z_charwise, "hello world", "\"zyw$\"zp");

// Paste from different registers in sequence
neovim_test!(scenarios, paste_reg_a_then_b, "hello world", "\"ayww\"byw$\"ap\"bp");
neovim_test!(scenarios, paste_reg_overwrite, "hello world", "\"aywdw\"ap");

// ═══════════════════════════════════════════════════════════════════════════════
// PASTE FROM SPECIAL REGISTERS
// ═══════════════════════════════════════════════════════════════════════════════

// Register 0 (last yank)
neovim_test!(scenarios, paste_reg_0, "hello world", "ywdw\"0p");
neovim_test!(scenarios, paste_reg_0_after_delete, "hello world test", "ywdw\"0p");

// Blackhole register (nothing to paste)
neovim_test!(scenarios, paste_blackhole_after_delete, "hello world", "\"_dwp");

// Unnamed register interactions
neovim_test!(scenarios, paste_unnamed_after_yank, "hello world", "ywp");
neovim_test!(scenarios, paste_unnamed_after_delete, "hello world", "dwp");
neovim_test!(scenarios, paste_unnamed_after_change, "hello world", "cwX<Esc>p");
neovim_test!(scenarios, paste_unnamed_after_x, "hello", "xp");
neovim_test!(scenarios, paste_unnamed_after_s, "hello", "sX<Esc>p");

// ═══════════════════════════════════════════════════════════════════════════════
// NUMBERED REGISTER PASTE
// ═══════════════════════════════════════════════════════════════════════════════

// Numbered registers 1-9 (recent deletes)
neovim_test!(scenarios, paste_reg_1, "l1\nl2\nl3", "dd\"1p");
neovim_test!(scenarios, paste_reg_1_after_two_dd, "l1\nl2\nl3\nl4", "dddd\"1p");
neovim_test!(scenarios, paste_reg_2_after_two_dd, "l1\nl2\nl3\nl4", "dddd\"2p");
neovim_test!(scenarios, paste_reg_1_chain, "l1\nl2\nl3\nl4", "ddddddjj\"1p\"2p");

// Small delete register
neovim_test!(scenarios, paste_small_delete, "hello world", "dw\"-p");
neovim_test!(scenarios, paste_small_x, "hello", "x\"-p");

// ═══════════════════════════════════════════════════════════════════════════════
// PASTE CHARWISE INTO VARIOUS CONTEXTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, paste_p_into_empty_buffer, "", "p");
neovim_test!(scenarios, paste_P_into_empty_buffer, "", "P");
neovim_test!(scenarios, paste_p_after_dd_empty, "hello", "ddp");
neovim_test!(scenarios, paste_P_after_dd_empty, "hello", "ddP");
neovim_test!(scenarios, paste_p_unicode, "日本語 hello", "ywwp");
neovim_test!(scenarios, paste_P_unicode, "日本語 hello", "yw$P");
neovim_test!(scenarios, paste_p_into_single_char, "a", "ylp");
neovim_test!(scenarios, paste_p_newline, "hello\nworld", "ywjp");

// ═══════════════════════════════════════════════════════════════════════════════
// PASTE LINEWISE INTO VARIOUS CONTEXTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, paste_linewise_into_empty, "", "p");
neovim_test!(scenarios, paste_linewise_single_line, "hello", "yyp");
neovim_test!(scenarios, paste_linewise_middle, "l1\nl2\nl3", cursor(1, 0), "yyp");
neovim_test!(scenarios, paste_linewise_last, "l1\nl2\nl3", cursor(2, 0), "yyp");
neovim_test!(scenarios, paste_linewise_first_P, "l1\nl2\nl3", "yyP");

// ═══════════════════════════════════════════════════════════════════════════════
// PASTE AFTER VISUAL DELETE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, paste_after_v_delete, "hello world", "vwdp");
neovim_test!(scenarios, paste_after_V_delete, "hello\nworld", "Vdp");
neovim_test!(scenarios, paste_after_v_yank, "hello world", "vwywp");
neovim_test!(scenarios, paste_after_V_yank, "hello\nworld", "Vyjp");

// ═══════════════════════════════════════════════════════════════════════════════
// xp — SWAP CHARACTERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, xp_swap, "hello", cursor(0, 1), "xp");
neovim_test!(scenarios, xp_at_start, "hello", "xp");
neovim_test!(scenarios, xp_at_end, "hello", cursor(0, 3), "xp");
neovim_test!(scenarios, xp_two_chars, "ab", "xp");
neovim_test!(scenarios, xp_unicode, "日本", "xp");

// ═══════════════════════════════════════════════════════════════════════════════
// ddp — SWAP LINES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ddp_swap_lines, "line1\nline2", "ddp");
neovim_test!(scenarios, ddp_at_last, "line1\nline2", cursor(1, 0), "ddp");
neovim_test!(scenarios, ddp_middle, "l1\nl2\nl3", cursor(1, 0), "ddp");

// ═══════════════════════════════════════════════════════════════════════════════
// PASTE WITH DOT REPEAT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, paste_dot_p, "hello", "ywp.");
neovim_test!(scenarios, paste_dot_P, "hello", "ywP.");
neovim_test!(scenarios, paste_dot_linewise, "hello\nworld", "yyp.");
neovim_test!(scenarios, paste_dot_count, "hello", "yw3p.");

// ═══════════════════════════════════════════════════════════════════════════════
// PASTE WITH UNDO
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, paste_undo_p, "hello", "ywpu");
neovim_test!(scenarios, paste_undo_P, "hello", "ywPu");
neovim_test!(scenarios, paste_undo_linewise, "hello\nworld", "yypu");
neovim_test!(scenarios, paste_undo_redo, "hello", "ywpu<C-r>");
neovim_test!(scenarios, paste_undo_multiple, "hello", "ywppuu");

// ═══════════════════════════════════════════════════════════════════════════════
// PASTE MIXED LINEWISE/CHARWISE
// ═══════════════════════════════════════════════════════════════════════════════

// Yank word (charwise) then line (linewise) — unnamed holds last
neovim_test!(scenarios, paste_charwise_then_linewise, "hello\nworld", "ywjyyp");
neovim_test!(scenarios, paste_linewise_then_charwise, "hello\nworld", "yyjywp");

// Delete word then delete line — numbered registers
neovim_test!(scenarios, paste_dw_dd_reg1, "hello\nworld\ntest", "dwjdd\"1p");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL PASTE (REPLACE SELECTION)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, visual_paste_replace, "hello world", "ywwvep");
neovim_test!(scenarios, visual_paste_line_replace, "l1\nl2\nl3", "yyjVp");
neovim_test!(scenarios, visual_paste_charwise_over, "hello world test", "ywwvwp");
neovim_test!(scenarios, visual_paste_preserves_yank, "hello world test", "ywwvep$p");
