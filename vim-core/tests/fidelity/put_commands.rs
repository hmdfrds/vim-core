// Put command fidelity tests: gp, gP, ]p, [p
//
// Extended paste commands: paste with cursor-after, indent-adjusted paste.

// ═══════════════════════════════════════════════════════════════════════════════
// gp — PUT AFTER, CURSOR AFTER PASTED TEXT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(put_commands, gp_char, "hello world", "ywwgp");
neovim_test!(put_commands, gp_word, "hello world", "ywgp");
neovim_test!(put_commands, gp_line, "line1\nline2", "yygp");
neovim_test!(put_commands, gp_line_at_end, "line1\nline2", cursor(1, 0), "yygp");
neovim_test!(put_commands, gp_deleted_word, "hello world end", "dwwgp");
neovim_test!(put_commands, gp_single_char, "abc", "ylgp");
neovim_test!(put_commands, gp_multiple_lines, "l1\nl2\nl3", "2yygp");
neovim_test!(put_commands, gp_empty_line, "hello\n\nworld", "yyjgp");
neovim_test!(put_commands, gp_register, "hello world", "\"ayiw$\"agp");
neovim_test!(put_commands, gp_with_count, "hello", "yw3gp");

// ═══════════════════════════════════════════════════════════════════════════════
// gP — PUT BEFORE, CURSOR AFTER PASTED TEXT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(put_commands, gP_char, "hello world", "ywwgP");
neovim_test!(put_commands, gP_word, "hello world", "yw$gP");
neovim_test!(put_commands, gP_line, "line1\nline2", "yygP");
neovim_test!(put_commands, gP_line_at_beginning, "line1\nline2", "yygP");
neovim_test!(put_commands, gP_deleted_word, "hello world end", "dwwgP");
neovim_test!(put_commands, gP_single_char, "abc", "ylgP");
neovim_test!(put_commands, gP_multiple_lines, "l1\nl2\nl3", "2yygP");
neovim_test!(put_commands, gP_register, "hello world", "\"ayiww\"agP");
neovim_test!(put_commands, gP_with_count, "hello", "yw3gP");

// ═══════════════════════════════════════════════════════════════════════════════
// ]p — PUT WITH INDENT ADJUSTMENT (AFTER)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(put_commands, bracket_p_basic, "    hello\nworld", "yy]p");
neovim_test!(put_commands, bracket_p_indented, "    if true {\n        body\n    }", cursor(1, 0), "yy]p");
neovim_test!(put_commands, bracket_p_deeper_indent, "        deep\n    shallow", "yyj]p");
neovim_test!(put_commands, bracket_p_no_indent, "hello\nworld", "yy]p");
neovim_test!(put_commands, bracket_p_tabs, "\thello\nworld", "yy]p");

// ═══════════════════════════════════════════════════════════════════════════════
// [p — PUT WITH INDENT ADJUSTMENT (BEFORE)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(put_commands, bracket_P_basic, "    hello\nworld", "yy[p");
neovim_test!(put_commands, bracket_P_indented, "    if true {\n        body\n    }", cursor(1, 0), "yy[p");
neovim_test!(put_commands, bracket_P_deeper_indent, "        deep\n    shallow", "yyj[p");
neovim_test!(put_commands, bracket_P_no_indent, "hello\nworld", "yy[p");

// ═══════════════════════════════════════════════════════════════════════════════
// gp/gP CURSOR POSITION VERIFICATION
// ═══════════════════════════════════════════════════════════════════════════════

// After gp, cursor should be AFTER pasted text (not on last char like p)
neovim_test!(put_commands, gp_cursor_vs_p, "hello world", "yw$gp");
neovim_test!(put_commands, gP_cursor_vs_P, "hello world", "yw$gP");
neovim_test!(put_commands, gp_line_cursor_below, "line1\nline2\nline3", "yyjgp");
neovim_test!(put_commands, gP_line_cursor_above, "line1\nline2\nline3", "yyjgP");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(put_commands, gp_empty_register, "hello", "gp");
neovim_test!(put_commands, gP_empty_register, "hello", "gP");
neovim_test!(put_commands, gp_single_char_buffer, "a", "ylgp");
neovim_test!(put_commands, gP_single_char_buffer, "a", "ylgP");
neovim_test!(put_commands, gp_unicode, "日本語", "ywgp");
neovim_test!(put_commands, gP_unicode, "日本語", "yw$gP");
neovim_test!(put_commands, gp_after_dd, "l1\nl2\nl3", "ddgp");
neovim_test!(put_commands, gP_after_dd, "l1\nl2\nl3", "ddgP");
