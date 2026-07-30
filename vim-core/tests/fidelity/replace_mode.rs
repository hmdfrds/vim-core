// Replace mode fidelity tests.
//
// R enters replace (overtype) mode. Characters typed overwrite existing text.
// Backspace undoes the last replacement. Esc returns to normal mode.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC REPLACE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(replace_mode, R_single_char, "hello", "Rx<Esc>");
neovim_test!(replace_mode, R_multiple_chars, "hello", "Rxyz<Esc>");
neovim_test!(replace_mode, R_full_overwrite, "hello", "Rabcde<Esc>");
neovim_test!(replace_mode, R_past_eol, "hi", "Rabcde<Esc>");
neovim_test!(replace_mode, R_on_empty, "", "Rabc<Esc>");
neovim_test!(replace_mode, R_unicode_replace, "hello", "R日本語<Esc>");
neovim_test!(replace_mode, R_replace_with_newline, "hello world", "Rhi<CR>there<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// BACKSPACE IN REPLACE MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(replace_mode, R_backspace_restores, "hello", "Rxy<BS><Esc>");
neovim_test!(replace_mode, R_backspace_multiple, "hello", "Rxyz<BS><BS><Esc>");
neovim_test!(replace_mode, R_backspace_at_start, "hello", "R<BS><Esc>");
neovim_test!(replace_mode, R_backspace_past_eol, "hi", "Rabcd<BS><BS><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// REPLACE MODE + DOT REPEAT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(replace_mode, R_dot_repeat, "hello\nworld", "Rxy<Esc>j0.");
neovim_test!(replace_mode, R_dot_repeat_past_eol, "hi\nhi", "Rabcde<Esc>j0.");

// ═══════════════════════════════════════════════════════════════════════════════
// REPLACE MODE WITH COUNT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(replace_mode, R_with_count, "hello world", "3Rx<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// REPLACE MODE ESCAPE/EXIT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(replace_mode, R_escape_immediate, "hello", "R<Esc>");
neovim_test!(replace_mode, R_ctrl_c_exit, "hello", "Rxy<C-c>");
neovim_test!(replace_mode, R_ctrl_bracket_exit, "hello", "Rxy<C-[>");

// ═══════════════════════════════════════════════════════════════════════════════
// REPLACE MODE ACROSS LINES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(replace_mode, R_multiline, "hello\nworld", "Rhi there<CR>friend<Esc>");
neovim_test!(replace_mode, R_at_eol_extends, "hello", "Rxyz extra<Esc>");
neovim_test!(replace_mode, R_mid_line, "hello world", cursor(0, 5), "R_WORLD<Esc>");
neovim_test!(replace_mode, R_last_char, "hello", cursor(0, 4), "RX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// REPLACE MODE WITH SPECIAL KEYS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(replace_mode, R_tab, "hello", "R\t<Esc>");
neovim_test!(replace_mode, R_backspace_at_bol, "hello\nworld", cursor(1, 0), "R<BS><Esc>");
neovim_test!(replace_mode, R_backspace_chain, "hello", "Rabc<BS><BS><BS><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// REPLACE MODE CROSS-LINE BACKSPACE
// ═══════════════════════════════════════════════════════════════════════════════

// Enter in replace mode inserts newline, backspace joins back
neovim_test!(replace_mode, R_enter_then_backspace, "hello world", "R<CR><BS><Esc>");
// Type chars, newline, then backspace across the line boundary
neovim_test!(replace_mode, R_type_enter_backspace, "hello world", "Rxy<CR><BS><Esc>");
// Multiple enters then backspaces
neovim_test!(replace_mode, R_multi_enter_backspace, "hello\nworld\nfoo", "R<CR><CR><BS><BS><Esc>");
// Type on new line, then backspace past the line boundary
neovim_test!(replace_mode, R_type_after_enter_then_backspace, "hello world", "R<CR>abc<BS><BS><BS><BS><Esc>");
// Enter at end of line (past EOL), then backspace
neovim_test!(replace_mode, R_enter_at_eol_backspace, "hi", cursor(0, 1), "R<CR><BS><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// REPLACE MODE WITH COUNT (3R)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(replace_mode, R_count_2, "hello world", "2Rx<Esc>");
neovim_test!(replace_mode, R_count_3_multi, "hello world", "3Rab<Esc>");
neovim_test!(replace_mode, R_count_on_short, "hi", "5Rx<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// REPLACE MODE + DOT REPEAT (MORE CASES)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(replace_mode, R_dot_3_lines, "aaa\nbbb\nccc", "Rxyz<Esc>j0.j0.");
neovim_test!(replace_mode, R_dot_preserves_text, "hello\nhello", "Rworld<Esc>j0.");
neovim_test!(replace_mode, R_dot_after_other_cmd, "hello\nworld", "RXX<Esc>jx.");

// ═══════════════════════════════════════════════════════════════════════════════
// REPLACE MODE + UNDO
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(replace_mode, R_undo_full, "hello", "Rworld<Esc>u");
neovim_test!(replace_mode, R_undo_partial, "hello world", "Rxyz<Esc>u");
neovim_test!(replace_mode, R_undo_then_redo, "hello", "Rxyz<Esc>u<C-r>");

// ═══════════════════════════════════════════════════════════════════════════════
// SINGLE REPLACE (r) — EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(replace_mode, r_at_eol, "hello", cursor(0, 4), "rX");
neovim_test!(replace_mode, r_at_bol, "hello", "rX");
neovim_test!(replace_mode, r_with_count, "hello", "3rX");
neovim_test!(replace_mode, r_count_exceeds, "hi", "5rX");
neovim_test!(replace_mode, r_newline, "hello world", "r<CR>");
neovim_test!(replace_mode, r_unicode_to_ascii, "日本語", "ra");
neovim_test!(replace_mode, r_ascii_to_unicode, "hello", "r日");
neovim_test!(replace_mode, r_escape_cancels, "hello", "r<Esc>");
neovim_test!(replace_mode, r_space, "hello", "r ");
neovim_test!(replace_mode, r_dot_repeat, "hello world", "rXw.");
neovim_test!(replace_mode, r_undo, "hello", "rXu");
neovim_test!(replace_mode, r_visual, "hello", "vlrX");
neovim_test!(replace_mode, r_visual_line, "hello\nworld", "VjrX");
