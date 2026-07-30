// Insert mode fidelity tests for vim-core.
//
// Tests for insert mode entry, text input, and exit.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC INSERT ENTRY
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(insert, i_basic, "hello", "i<Esc>");
neovim_test!(insert, i_insert_char, "hello", "iX<Esc>");
neovim_test!(insert, i_insert_multiple, "hello", "iXYZ<Esc>");

neovim_test!(insert, a_basic, "hello", "a<Esc>");
neovim_test!(insert, a_append_char, "hello", "aX<Esc>");
neovim_test!(insert, a_at_eol, "hello", cursor(0, 4), "aX<Esc>");

neovim_test!(insert, I_line_start, "  hello", "I<Esc>");
neovim_test!(insert, I_insert_char, "  hello", "IX<Esc>");

neovim_test!(insert, A_line_end, "hello", "A<Esc>");
neovim_test!(insert, A_append_char, "hello", "AX<Esc>");

// NEW: More insert entry edge cases
neovim_test!(insert, i_empty_buffer, "", "i<Esc>");
neovim_test!(insert, a_empty_buffer, "", "a<Esc>");
neovim_test!(insert, I_empty_buffer, "", "I<Esc>");
neovim_test!(insert, A_empty_buffer, "", "A<Esc>");
neovim_test!(insert, i_single_char, "a", "iX<Esc>");
neovim_test!(insert, a_single_char, "a", "aX<Esc>");
neovim_test!(insert, i_at_eol, "hello", cursor(0, 4), "iX<Esc>");
neovim_test!(insert, I_no_indent, "hello", "IX<Esc>");
neovim_test!(insert, I_all_whitespace, "    ", "IX<Esc>");
neovim_test!(insert, A_single_char, "a", "AX<Esc>");
neovim_test!(insert, i_unicode, "日本語", "iX<Esc>");
neovim_test!(insert, a_unicode, "日本語", cursor(0, 2), "aX<Esc>");
neovim_test!(insert, I_unicode, "  日本語", "IX<Esc>");
neovim_test!(insert, A_unicode, "日本語", "AX<Esc>");
neovim_test!(insert, i_insert_unicode, "hello", "i日本語<Esc>");
neovim_test!(insert, a_append_unicode, "hello", "a日本語<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// OPEN NEW LINE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(insert, o_basic, "hello", "o<Esc>");
neovim_test!(insert, o_with_text, "hello", "oworld<Esc>");
neovim_test!(insert, o_indented, "  hello", "o<Esc>");

neovim_test!(insert, O_basic, "hello", "O<Esc>");
neovim_test!(insert, O_with_text, "hello", "Oworld<Esc>");
neovim_test!(insert, O_at_first_line, "hello", "Oabove<Esc>");

// NEW: More open line edge cases
neovim_test!(insert, o_empty_buffer, "", "o<Esc>");
neovim_test!(insert, O_empty_buffer, "", "O<Esc>");
neovim_test!(insert, o_with_unicode, "hello", "o日本語<Esc>");
neovim_test!(insert, O_with_unicode, "hello", "O日本語<Esc>");
neovim_test!(insert, o_multiline, "line1\nline2\nline3", cursor(1, 0), "onew<Esc>");
neovim_test!(insert, O_multiline, "line1\nline2\nline3", cursor(1, 0), "Onew<Esc>");
neovim_test!(insert, o_last_line, "line1\nline2", cursor(1, 0), "onew<Esc>");
neovim_test!(insert, O_first_line, "line1\nline2", "Onew<Esc>");
neovim_test!(insert, o_preserve_indent, "  hello\n  world", "onew<Esc>");
neovim_test!(insert, O_preserve_indent, "  hello\n  world", cursor(1, 0), "Onew<Esc>");
neovim_test!(insert, o_tabs_indent, "\thello", "onew<Esc>");
neovim_test!(insert, O_tabs_indent, "\thello", "Onew<Esc>");
neovim_test!(insert, o_multiple_lines, "hello", "oone\ntwo<Esc>");
neovim_test!(insert, O_multiple_lines, "hello", "Oone\ntwo<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT WITH COUNT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(insert, i_with_count, "hello", "3iX<Esc>");
neovim_test!(insert, a_with_count, "hello", "3aX<Esc>");
neovim_test!(insert, o_with_count, "hello", "3oX<Esc>");

// NEW: More count edge cases
neovim_test!(insert, i_count_multiple_chars, "hello", "3iab<Esc>");
neovim_test!(insert, a_count_multiple_chars, "hello", "3aab<Esc>");
neovim_test!(insert, I_with_count, "  hello", "3IX<Esc>");
neovim_test!(insert, A_with_count, "hello", "3AX<Esc>");
neovim_test!(insert, O_with_count, "hello", "3OX<Esc>");
neovim_test!(insert, i_count_1, "hello", "1iX<Esc>");
neovim_test!(insert, i_count_0, "hello", "0iX<Esc>");
neovim_test!(insert, i_count_large, "hello", "5iab<Esc>");
neovim_test!(insert, o_count_multiple_chars, "hello", "2oab<Esc>");
neovim_test!(insert, i_count_unicode, "hello", "3i日<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// BACKSPACE AND DELETE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(insert, backspace_single, "hello", "A<BS><Esc>");
neovim_test!(insert, backspace_multiple, "hello", "A<BS><BS><Esc>");
neovim_test!(insert, backspace_at_start, "hello", "i<BS><Esc>");
neovim_test!(insert, backspace_into_prev_line, "hello\nworld", cursor(1, 0), "i<BS><Esc>");

neovim_test!(insert, delete_word, "hello world", "A<C-w><Esc>");
neovim_test!(insert, delete_line, "hello world", "A<C-u><Esc>");

// NEW: More backspace/delete edge cases
neovim_test!(insert, backspace_empty_buffer, "", "i<BS><Esc>");
neovim_test!(insert, backspace_single_char, "a", "A<BS><Esc>");
neovim_test!(insert, backspace_all_chars, "ab", "A<BS><BS><Esc>");
neovim_test!(insert, backspace_unicode, "日本語", "A<BS><Esc>");
neovim_test!(insert, backspace_multiple_unicode, "日本語", "A<BS><BS><Esc>");
neovim_test!(insert, backspace_mixed, "hello日本語", "A<BS><BS><BS><Esc>");
neovim_test!(insert, delete_word_middle, "hello world test", cursor(0, 11), "A<C-w><Esc>");
neovim_test!(insert, delete_word_single, "hello", "A<C-w><Esc>");
neovim_test!(insert, delete_word_unicode, "日本語 テスト", "A<C-w><Esc>");
neovim_test!(insert, delete_line_middle, "hello world", cursor(0, 6), "i<C-u><Esc>");
neovim_test!(insert, delete_line_empty, "", "i<C-u><Esc>");
neovim_test!(insert, backspace_after_insert, "hello", "iXYZ<BS><Esc>");
neovim_test!(insert, backspace_new_text_only, "hello", "iXYZ<BS><BS><BS><Esc>");
neovim_test!(insert, ctrl_h_backspace, "hello", "A<C-h><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// SPECIAL CHARACTER INSERT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(insert, enter_newline, "hello", "A<CR><Esc>");
neovim_test!(insert, tab_insert, "hello", "A<Tab><Esc>");

// NEW: More special character edge cases
neovim_test!(insert, enter_middle, "hello world", cursor(0, 5), "a<CR><Esc>");
neovim_test!(insert, enter_multiple, "hello", "A<CR><CR><Esc>");
neovim_test!(insert, enter_with_text, "hello", "A<CR>world<Esc>");
neovim_test!(insert, enter_empty_buffer, "", "i<CR><Esc>");
neovim_test!(insert, tab_multiple, "hello", "A<Tab><Tab><Esc>");
neovim_test!(insert, tab_at_start, "hello", "i<Tab><Esc>");
neovim_test!(insert, tab_empty_buffer, "", "i<Tab><Esc>");
neovim_test!(insert, space_insert, "hello", "A <Esc>");
neovim_test!(insert, multiple_spaces, "hello", "A   <Esc>");
neovim_test!(insert, enter_preserves_indent, "  hello", "A<CR>world<Esc>");
neovim_test!(insert, ctrl_m_newline, "hello", "A<C-m><Esc>");
neovim_test!(insert, ctrl_j_newline, "hello", "A<C-j><Esc>");
neovim_test!(insert, ctrl_i_tab, "hello", "A<C-i><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT MODE NAVIGATION
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(insert, arrow_left, "hello", "A<Left><Esc>");
neovim_test!(insert, arrow_right, "hello", "i<Right><Esc>");
neovim_test!(insert, home_key, "hello", cursor(0, 3), "i<Home><Esc>");
neovim_test!(insert, end_key, "hello", "i<End><Esc>");

// NEW: More navigation edge cases
neovim_test!(insert, arrow_up, "hello\nworld", cursor(1, 2), "i<Up><Esc>");
neovim_test!(insert, arrow_down, "hello\nworld", cursor(0, 2), "i<Down><Esc>");
neovim_test!(insert, arrow_left_at_start, "hello", "i<Left><Esc>");
neovim_test!(insert, arrow_right_at_end, "hello", "A<Right><Esc>");
neovim_test!(insert, arrow_left_multiple, "hello", "A<Left><Left><Left><Esc>");
neovim_test!(insert, arrow_right_multiple, "hello", "i<Right><Right><Right><Esc>");
neovim_test!(insert, home_at_start, "hello", "i<Home><Esc>");
neovim_test!(insert, end_at_end, "hello", "A<End><Esc>");
neovim_test!(insert, arrow_left_wrap, "hello\nworld", cursor(1, 0), "i<Left><Esc>");
neovim_test!(insert, ctrl_left_word, "hello world", "A<C-Left><Esc>");
neovim_test!(insert, ctrl_right_word, "hello world", "i<C-Right><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// ESCAPE VARIATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(insert, escape_normal, "hello", "i<Esc>");
neovim_test!(insert, ctrl_c_escape, "hello", "i<C-c>");
neovim_test!(insert, ctrl_bracket, "hello", "i<C-[>");

// NEW: More escape edge cases
neovim_test!(insert, escape_after_insert, "hello", "iXYZ<Esc>");
neovim_test!(insert, ctrl_c_after_insert, "hello", "iXYZ<C-c>");
neovim_test!(insert, escape_from_a, "hello", "a<Esc>");
neovim_test!(insert, escape_from_A, "hello", "A<Esc>");
neovim_test!(insert, escape_from_o, "hello", "o<Esc>");
neovim_test!(insert, escape_from_O, "hello", "O<Esc>");
neovim_test!(insert, double_escape, "hello", "i<Esc><Esc>");
neovim_test!(insert, escape_empty_buffer, "", "i<Esc>");
neovim_test!(insert, escape_cursor_position, "hello", cursor(0, 2), "iX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR POSITION AFTER INSERT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(insert, cursor_after_i, "hello", "iX<Esc>"); // cursor should be on X
neovim_test!(insert, cursor_after_a, "hello", "aX<Esc>"); // cursor should be on X
neovim_test!(insert, cursor_after_A, "hello", "AX<Esc>"); // cursor at end

// NEW: More cursor position tests
neovim_test!(insert, cursor_after_I, "  hello", "IX<Esc>");
neovim_test!(insert, cursor_after_o, "hello", "oX<Esc>");
neovim_test!(insert, cursor_after_O, "hello", "OX<Esc>");
neovim_test!(insert, cursor_after_multiple_chars, "hello", "iXYZ<Esc>");
neovim_test!(insert, cursor_after_backspace, "hello", "AX<BS><Esc>");
neovim_test!(insert, cursor_after_enter, "hello", "A<CR>X<Esc>");
neovim_test!(insert, cursor_i_empty, "", "iX<Esc>");
neovim_test!(insert, cursor_a_empty, "", "aX<Esc>");
neovim_test!(insert, cursor_o_empty, "", "oX<Esc>");
neovim_test!(insert, cursor_O_empty, "", "OX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// SUBSTITUTE / REPLACE COMMANDS (s, S, r, R)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(insert, s_substitute, "hello", "sX<Esc>");
neovim_test!(insert, s_substitute_count, "hello", "3sX<Esc>");
neovim_test!(insert, S_substitute_line, "hello world", "SX<Esc>");
neovim_test!(insert, r_replace, "hello", "rX");
neovim_test!(insert, r_replace_eol, "hello", cursor(0, 4), "rX");
neovim_test!(insert, R_replace_mode, "hello", "RXY<Esc>");
neovim_test!(insert, r_replace_empty, "", "rX");
neovim_test!(insert, r_replace_unicode, "hello", "r日");
neovim_test!(insert, R_replace_unicode, "hello", "R日本<Esc>");
neovim_test!(insert, s_at_eol, "hello", cursor(0, 4), "sX<Esc>");
neovim_test!(insert, S_empty_buffer, "", "SX<Esc>");
neovim_test!(insert, s_empty_buffer, "", "sX<Esc>");
neovim_test!(insert, R_overwrite_all, "hello", "Rabcde<Esc>");
neovim_test!(insert, R_extend_past_eol, "hello", "Rabcdefgh<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// GI (GO TO LAST INSERT POSITION)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(insert, gi_return_to_insert, "hello world", "iX<Esc>$giY<Esc>");
neovim_test!(insert, gi_after_o, "hello", "oX<Esc>kgiY<Esc>");
neovim_test!(insert, gi_after_a, "hello", "aX<Esc>0giY<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT MODE WITH OPERATORS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(insert, cw_then_insert, "hello world", "cwbye<Esc>");
neovim_test!(insert, cc_then_insert, "hello world", "ccnew<Esc>");
neovim_test!(insert, C_then_insert, "hello world", cursor(0, 5), "Cend<Esc>");
neovim_test!(insert, ciw_then_insert, "hello world", cursor(0, 6), "ciwbye<Esc>");
neovim_test!(insert, ci_paren_insert, "(hello)", cursor(0, 3), "ci(bye<Esc>");
neovim_test!(insert, ci_quote_insert, "\"hello\"", cursor(0, 3), "ci\"bye<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT WITH INSERT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(insert, dot_repeat_insert, "hello world", "iX<Esc>w.");
neovim_test!(insert, dot_repeat_append, "hello world", "aX<Esc>w.");
neovim_test!(insert, dot_repeat_o, "hello", "oX<Esc>.");
neovim_test!(insert, dot_repeat_cw, "one two three", "cwnew<Esc>w.");
neovim_test!(insert, dot_repeat_s, "hello", "sX<Esc>l.");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES - COMPREHENSIVE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(insert, i_at_very_long_line, "abcdefghijklmnopqrstuvwxyz", cursor(0, 13), "iX<Esc>");
neovim_test!(insert, a_at_very_long_line, "abcdefghijklmnopqrstuvwxyz", cursor(0, 13), "aX<Esc>");
neovim_test!(insert, enter_long_text, "hello", "Aworld world world<Esc>");
neovim_test!(insert, backspace_entire_insert, "hello", "iXYZ<BS><BS><BS><Esc>");
neovim_test!(insert, mixed_unicode_ascii, "hello", "i日aテbスcト<Esc>");
neovim_test!(insert, emoji_insert, "hello", "i👍<Esc>");
neovim_test!(insert, combining_chars, "hello", "ie\u{0301}<Esc>");
// ZWJ sequences can cause Neovim oracle to hang - skipped
// neovim_test!(insert, zwj_sequence, "hello", "i👨\\u{200D}👩\\u{200D}👧<Esc>");
neovim_test!(insert, i_multiline_insert, "hello", "iline1\nline2\nline3<Esc>");
neovim_test!(insert, o_then_backspace, "hello", "oX<BS><Esc>");
neovim_test!(insert, o_bs_through_indent, "  hello", "o<BS><BS><Esc>");
neovim_test!(insert, o_bs_all_indent_then_esc, "    hello", "o<BS><BS><BS><BS><Esc>");
neovim_test!(insert, ctrl_o_normal_cmd, "hello world", "i<C-o>w<Esc>");
neovim_test!(insert, ctrl_r_register, "hello world", "ywi<C-r>0<Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Insert Mode Indent/Outdent (Ctrl-T, Ctrl-D)
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(insert, insert_ctrl_t_basic, "hello", "I<C-t><Esc>");
neovim_test!(insert, insert_ctrl_t_twice, "hello", "I<C-t><C-t><Esc>");
neovim_test!(insert, insert_ctrl_t_then_type, "world", "I<C-t>hello <Esc>");
neovim_test!(insert, insert_ctrl_d_basic, "    hello", "I<C-d><Esc>");
neovim_test!(insert, insert_ctrl_d_at_zero, "hello", "I<C-d><Esc>");
neovim_test!(insert, insert_ctrl_t_then_d, "hello", "I<C-t><C-d><Esc>");
neovim_test!(insert, insert_ctrl_d_twice, "        hello", "I<C-d><C-d><Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Insert Mode Delete Forward (Delete key)
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(insert, insert_delete_key, "hello", "i<Del><Esc>");
neovim_test!(insert, insert_delete_at_eol, "hello", "A<Del><Esc>");
neovim_test!(insert, insert_delete_mid_word, "hello", "lli<Del><Esc>");
neovim_test!(insert, insert_delete_empty, "", "i<Del><Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Insert Mode Ctrl-A (re-insert last inserted text)
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(insert, insert_ctrl_a_basic, "hello", "aXY<Esc>o<C-a><Esc>");
neovim_test!(insert, insert_ctrl_a_after_word, "test", "iworld <Esc>A<C-a><Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Insert Mode Ctrl-W edge cases
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(insert, insert_ctrl_w_at_bol, "hello\nworld", "jo<C-w><Esc>");
neovim_test!(insert, insert_ctrl_w_only_spaces, "hello   ", "A<C-w><Esc>");
neovim_test!(insert, insert_ctrl_u_at_bol, "hello\nworld", "jI<C-u><Esc>");
neovim_test!(insert, insert_ctrl_u_mid_line, "hello world", "ea<C-u><Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Insert Mode Ctrl-O (one-shot normal)
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(insert, insert_ctrl_o_motion, "hello world", "i<C-o>w<Esc>");
neovim_test!(insert, insert_ctrl_o_delete, "hello world", "i<C-o>dw<Esc>");
neovim_test!(insert, insert_ctrl_o_append, "hello world test", "i<C-o>$end<Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Insert Mode Ctrl-V (insert literal character)
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(insert, insert_ctrl_v_escape, "hello", "i<C-v><Esc><Esc>");
neovim_test!(insert, insert_ctrl_v_tab, "hello", "A<C-v><Tab><Esc>");
neovim_test!(insert, insert_ctrl_v_cr, "hello", "A<C-v><CR><Esc>");
neovim_test!(insert, insert_ctrl_v_digit, "hello", "A<C-v>065<Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Insert Mode Ctrl-Y / Ctrl-E (copy char from above/below)
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(insert, insert_ctrl_y_basic, "abcdef\n", cursor(1, 0), "i<C-y><Esc>");
neovim_test!(insert, insert_ctrl_y_multiple, "abcdef\n", cursor(1, 0), "i<C-y><C-y><C-y><Esc>");
neovim_test!(insert, insert_ctrl_y_no_line_above, "hello", "i<C-y><Esc>");
neovim_test!(insert, insert_ctrl_y_at_eol_above, "hi\n", cursor(1, 0), "i<C-y><C-y><C-y><Esc>");
neovim_test!(insert, insert_ctrl_e_basic, "\nabcdef", "i<C-e><Esc>");
neovim_test!(insert, insert_ctrl_e_multiple, "\nabcdef", "i<C-e><C-e><C-e><Esc>");
neovim_test!(insert, insert_ctrl_e_no_line_below, "hello", "i<C-e><Esc>");
neovim_test!(insert, insert_ctrl_e_at_eol_below, "\nhi", "i<C-e><C-e><C-e><Esc>");
neovim_test!(insert, insert_ctrl_y_unicode, "日本語\n", cursor(1, 0), "i<C-y><Esc>");
neovim_test!(insert, insert_ctrl_e_unicode, "\n日本語", "i<C-e><Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Insert Mode Ctrl-R (insert from register) — MORE CASES
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(insert, ctrl_r_named_reg, "hello world", "\"ayiwwi<C-r>a<Esc>");
neovim_test!(insert, ctrl_r_yank_reg, "hello world", "yiwwi<C-r>0<Esc>");
neovim_test!(insert, ctrl_r_expr_simple, "hello", "i<C-r>=42<CR><Esc>");
neovim_test!(insert, ctrl_r_search_reg, "hello", "/hello<CR>i<C-r>/<Esc>");
neovim_test!(insert, ctrl_r_file_reg, "hello", "i<C-r>%<Esc>");
neovim_test!(insert, ctrl_r_clipboard, "hello", "yywi<C-r>\"<Esc>");

// Ctrl-R special registers (word/WORD/line under cursor)
neovim_test!(insert, ctrl_r_ctrl_w_word, "hello world", "ea<C-r><C-w><Esc>");
neovim_test!(insert, ctrl_r_ctrl_l_line, "hello world", "o<C-r><C-l><Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Insert Mode Ctrl-O (one-shot normal) — MORE CASES
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(insert, ctrl_o_find, "hello world", "i<C-o>fwX<Esc>");
neovim_test!(insert, ctrl_o_change_case, "hello world", "i<C-o>gUwX<Esc>");
neovim_test!(insert, ctrl_o_yank, "hello world", "i<C-o>ywhello <Esc>");
neovim_test!(insert, ctrl_o_jump_line, "hello\nworld", "i<C-o>jX<Esc>");
neovim_test!(insert, ctrl_o_indent, "hello", "i<C-o>>><Esc>");
neovim_test!(insert, ctrl_o_search, "hello world hello", "i<C-o>/world<CR>X<Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// gi (return to last insert position) — MORE CASES
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(insert, gi_after_cw, "hello world", "cwbye<Esc>$giX<Esc>");
neovim_test!(insert, gi_after_cc, "hello\nworld", "ccnew<Esc>jgiX<Esc>");
neovim_test!(insert, gi_after_s, "hello", "sX<Esc>$giY<Esc>");
neovim_test!(insert, gi_with_count, "hello", "iX<Esc>$3giY<Esc>");
neovim_test!(insert, gi_no_prior_insert, "hello", "giX<Esc>");
neovim_test!(insert, gi_after_O, "hello\nworld", "OX<Esc>GgiY<Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Insert Mode Ctrl-K (digraph insertion)
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(insert, digraph_ctrl_k_basic, "", "i<C-k>a*<Esc>");
neovim_test!(insert, digraph_ctrl_k_midtext, "hello", "A <C-k>a*<Esc>");
neovim_test!(insert, digraph_ctrl_k_n_tilde, "", "i<C-k>n~<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CTRL-V LITERAL INSERT (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// Ctrl-V then 'a' inserts 'a' literally
neovim_test!(insert, ctrl_v_literal_char, "hello", "i<C-v>a<Esc>");

// Ctrl-V then '(' inserts '(' without auto-close
neovim_test!(insert, ctrl_v_literal_bracket, "x", "i<C-v>(<Esc>");

// Bug hunt: S on indented line
neovim_test!(insert, S_indented_line, "    hello world\n", "Sreplaced<Esc>");

// Bug hunt: Ctrl-W at BOL
neovim_test!(insert, ctrl_w_at_bol, "first\nsecond\n", cursor(1, 0), "i<C-w><Esc>");

// Bug hunt: gi without prior insert
neovim_test!(insert, gi_no_prior_insert_cursor, "hello\n", "gi<Esc>");

// Bug hunt: arrow keys break dot-repeat
neovim_test!(insert, arrow_breaks_repeat, "hello\nworld\n", "ihello<Left>X<Esc>j.");

// Bug hunt: Ctrl-A (insert last inserted) during NativeInsert-like session
// In Vim: type "hello", Esc, then i Ctrl-A should insert "hello"
neovim_test!(insert, ctrl_a_after_insert, "world\n", "ihello<Esc>ji<C-a><Esc>");

// Ctrl-O (one-shot normal) breaks the repeat block like arrow keys
neovim_test!(insert, ctrl_o_breaks_repeat, "hello\nworld\n", "ihello<C-o>lX<Esc>j.");

// S on indented line + type + undo restores cursor to pre-S position
neovim_test!(insert, S_type_undo, "    hello\n", "Snew<Esc>u");
