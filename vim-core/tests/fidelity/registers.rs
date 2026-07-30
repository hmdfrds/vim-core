// Register fidelity tests for vim-core.
//
// Tests for yank/delete registers and their usage with put commands.
// Registers store text for later use.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC YANK TO NAMED REGISTER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(registers, yank_word_to_reg_a, "hello world", "\"ayiw");
neovim_test!(registers, yank_line_to_reg_b, "hello\nworld", "\"byy");

// NEW: More yank to register cases
neovim_test!(registers, yank_to_reg_c, "hello world", "\"cyiw");
neovim_test!(registers, yank_to_reg_z, "hello world", "\"zyiw");
neovim_test!(registers, yank_unicode_to_reg, "日本語", "\"ayiw");
neovim_test!(registers, yank_line_to_reg_a, "hello world", "\"ayy");
neovim_test!(registers, yank_to_eol_to_reg, "hello world", cursor(0, 6), "\"ay$");
neovim_test!(registers, yank_multiline_to_reg, "line1\nline2\nline3", "\"aVjy");
neovim_test!(registers, yank_word_motion_to_reg, "hello world test", "\"ayw");

// ═══════════════════════════════════════════════════════════════════════════════
// DELETE TO NAMED REGISTER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(registers, delete_word_to_reg_a, "hello world", "\"adiw");
neovim_test!(registers, delete_line_to_reg_b, "hello\nworld", "\"bdd");

// NEW: More delete to register cases
neovim_test!(registers, delete_to_reg_c, "hello world", "\"cdiw");
neovim_test!(registers, delete_to_reg_z, "hello world", "\"zdiw");
neovim_test!(registers, delete_unicode_to_reg, "日本語 テスト", "\"adiw");
neovim_test!(registers, delete_to_eol_to_reg, "hello world", cursor(0, 6), "\"aD");
neovim_test!(registers, delete_char_to_reg, "hello", "\"ax");
neovim_test!(registers, delete_multiline_to_reg, "line1\nline2\nline3", "\"aVjd");
neovim_test!(registers, delete_word_motion_to_reg, "hello world test", "\"adw");

// ═══════════════════════════════════════════════════════════════════════════════
// PUT FROM NAMED REGISTER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(registers, put_from_reg_a, "hello world", "\"ayiw$\"ap");
neovim_test!(registers, put_before_from_reg_a, "hello world", "\"ayiw$\"aP");

// NEW: More put from register cases
neovim_test!(registers, put_from_reg_z, "hello world", "\"zyiw$\"zp");
neovim_test!(registers, put_line_from_reg, "hello\nworld", "\"ayy$\"ap");
neovim_test!(registers, put_unicode_from_reg, "日本語", "\"ayiw$\"ap");
neovim_test!(registers, put_multiple_times, "hello world", "\"ayiw$\"ap\"ap");
neovim_test!(registers, put_before_unicode, "日本語", "\"ayiw$\"aP");
neovim_test!(registers, put_linewise_after, "line1\nline2", "\"ayy\"ap");
neovim_test!(registers, put_linewise_before, "line1\nline2", "\"ayy\"aP");

// ═══════════════════════════════════════════════════════════════════════════════
// UNNAMED REGISTER (DEFAULT)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(registers, yank_uses_unnamed, "hello", "ywp");
neovim_test!(registers, delete_uses_unnamed, "hello", "dwp");

// NEW: More unnamed register cases
neovim_test!(registers, yank_overwrites_unnamed, "hello world", "yiwwdiw0p");
neovim_test!(registers, delete_overwrites_unnamed, "hello world", "diwwywP");
neovim_test!(registers, unnamed_with_dd, "line1\nline2", "ddp");
neovim_test!(registers, unnamed_with_yy, "line1\nline2", "yyp");
neovim_test!(registers, unnamed_with_x, "hello", "xp");
neovim_test!(registers, unnamed_unicode, "日本語", "yiwp");
neovim_test!(registers, unnamed_explicit, "hello", "\"\"yiwp");

// ═══════════════════════════════════════════════════════════════════════════════
// UPPERCASE REGISTERS (APPEND)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(registers, append_to_reg, "ab", "\"ayiw$\"Ayiw");

// NEW: More append register cases
neovim_test!(registers, append_word_to_reg, "hello world", "\"ayiww\"Ayiw$\"ap");
neovim_test!(registers, append_line_to_reg, "line1\nline2", "\"ayyjw\"Ayy0\"ap");
neovim_test!(registers, append_unicode_to_reg, "日本語 テスト", "\"ayiww\"Ayiw$\"ap");
neovim_test!(registers, append_multiple_times, "one two three", "\"ayiww\"Ayiww\"Ayiw$\"ap");
neovim_test!(registers, append_B_to_reg, "hello world", "\"byww\"Byw$\"bp");

// ═══════════════════════════════════════════════════════════════════════════════
// SPECIAL REGISTERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(registers, yank_to_zero_reg, "hello", "yy\"0p");
neovim_test!(registers, blackhole_delete, "hello", "\"_dw");

// NEW: More special register cases
neovim_test!(registers, zero_reg_not_affected_by_delete, "hello world", "yiwdw\"0p");
neovim_test!(registers, blackhole_yank, "hello", "\"_yw");
neovim_test!(registers, blackhole_delete_line, "hello\nworld", "\"_dd");
neovim_test!(registers, blackhole_change, "hello", "\"_cwX<Esc>");
neovim_test!(registers, blackhole_x, "hello", "\"_x");
neovim_test!(registers, plus_register, "hello", "\"+yy");
neovim_test!(registers, star_register, "hello", "\"*yy");
neovim_test!(registers, small_delete_reg, "hello", "x\"1p");
neovim_test!(registers, expression_reg, "hello", "\"=");

// ═══════════════════════════════════════════════════════════════════════════════
// NUMBERED REGISTERS (1-9)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(registers, delete_shifts_numbered, "line1\nline2\nline3", "dd\"1p");
neovim_test!(registers, numbered_reg_1, "line1\nline2", "dd\"1p");
neovim_test!(registers, numbered_reg_chain, "l1\nl2\nl3", "ddjdd\"2p");
neovim_test!(registers, yank_does_not_shift_numbered, "hello world", "yw\"1p");

// ═══════════════════════════════════════════════════════════════════════════════
// REGISTER WITH VISUAL MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(registers, visual_yank_to_reg, "hello world", "viw\"ay");
neovim_test!(registers, visual_delete_to_reg, "hello world", "viw\"ad");
neovim_test!(registers, visual_put_from_reg, "hello world", "viw\"aywviw\"ap");
neovim_test!(registers, visual_line_yank_to_reg, "line1\nline2", "V\"ay");
neovim_test!(registers, visual_line_delete_to_reg, "line1\nline2", "V\"ad");
neovim_test!(registers, visual_block_yank_to_reg, "abc\ndef", "<C-v>j\"ay");

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT MODE REGISTER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(registers, insert_from_reg, "hello world", "\"ayiwA<C-r>a<Esc>");
neovim_test!(registers, insert_unnamed_reg, "hello world", "yiwA<C-r>\"<Esc>");
neovim_test!(registers, insert_from_reg_literal, "hello", "yiwA<C-r><C-r>0<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(registers, empty_reg_put, "hello", "\"zp");
neovim_test!(registers, reg_with_newline, "hello\nworld", "\"ayy\"ap");
neovim_test!(registers, reg_unicode_emoji, "👍 test", "\"ayiw");
neovim_test!(registers, reg_cjk, "日本語テスト", "\"ayiw");
neovim_test!(registers, reg_mixed_content, "hello 日本語", "\"ayw");
neovim_test!(registers, swap_with_reg, "hello world", "yiwwviwp");
neovim_test!(registers, multiple_regs_same_op, "a b c", "\"ayiw w\"byiw w\"cyiw");
neovim_test!(registers, reg_after_undo, "hello", "\"ayiwdwu\"ap");
neovim_test!(registers, linewise_vs_charwise, "hello", "yy\"aywP\"ap");

// Pipeline hardening regressions: register semantics through mixed command flow
neovim_test!(registers, named_register_put_dot_repeat, "one two", "\"ayiw$\"ap.");
neovim_test!(registers, visual_delete_named_register_then_put, "one two three", "viw\"adw\"ap");
neovim_test!(registers, zero_register_survives_small_delete, "one two", "yiwx\"0p");

// ─────────────────────────────────────────────────────────────────────────────
// Read-Only Registers
// ─────────────────────────────────────────────────────────────────────────────

// ". register (last inserted text)
neovim_test!(registers, dot_reg_after_insert, "hello", "aXYZ<Esc>o<C-r>.<Esc>");
// "/ register (last search pattern)
neovim_test!(registers, slash_reg_after_search, "hello world", "/world<CR>o<C-r>/<Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Numbered Register Rotation (full chain)
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(registers, numbered_3_deep, "aaa\nbbb\nccc\nddd", "ddddddjj\"3p\"2p\"1p");
neovim_test!(registers, yank_does_not_shift, "aaa\nbbb", "yyddjp\"1p");
neovim_test!(registers, small_delete_reg_dw, "hello world", "dw\"-p");
neovim_test!(registers, small_delete_x, "hello", "x\"-p");

// ─────────────────────────────────────────────────────────────────────────────
// Register + Paste Edge Cases
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(registers, paste_overwrite_unnamed, "aaa bbb ccc", "ywwdw0p");
neovim_test!(registers, visual_put_replaces, "hello world", "yiwwviwp");
neovim_test!(registers, reg_a_append_lines, "aaa\nbbb\nccc", "\"ayyjj\"Ayygg\"ap");
