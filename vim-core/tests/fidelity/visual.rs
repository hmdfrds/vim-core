// Visual mode fidelity tests for vim-core.
//
// Tests for visual mode (v, V, Ctrl-V) and related operations.

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL MODE ENTRY
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual, v_enter, "hello", "v");
neovim_test!(visual, V_enter, "hello", "V");
neovim_test!(visual, ctrl_v_enter, "hello", "<C-v>");

// NEW: More entry edge cases
neovim_test!(visual, v_empty_buffer, "", "v");
neovim_test!(visual, V_empty_buffer, "", "V");
neovim_test!(visual, ctrl_v_empty_buffer, "", "<C-v>");
neovim_test!(visual, v_single_char, "a", "v");
neovim_test!(visual, V_single_line, "hello", "V");
neovim_test!(visual, ctrl_v_single_char, "a", "<C-v>");
neovim_test!(visual, v_unicode, "日本語", "v");
neovim_test!(visual, V_unicode_enter, "日本語", "V");
neovim_test!(visual, ctrl_v_unicode_enter, "日本語", "<C-v>");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL CHARACTER SELECTION
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual, v_select_right, "hello", "vl");
neovim_test!(visual, v_select_word, "hello world", "vw");
neovim_test!(visual, v_select_end, "hello", "v$");
neovim_test!(visual, v_select_multiple, "hello world foo", "v2w");

// NEW: More character selection edge cases
neovim_test!(visual, v_select_left, "hello", cursor(0, 4), "vh");
neovim_test!(visual, v_select_start, "hello", cursor(0, 4), "v0");
neovim_test!(visual, v_select_first_nonblank, "  hello", cursor(0, 6), "v^");
neovim_test!(visual, v_select_find, "hello world", "vfo");
neovim_test!(visual, v_select_till, "hello world", "vto");
neovim_test!(visual, v_select_F_back, "hello world", cursor(0, 10), "vFo");
neovim_test!(visual, v_select_T_back, "hello world", cursor(0, 10), "vTo");
neovim_test!(visual, v_select_e, "hello world", "ve");
neovim_test!(visual, v_select_b, "hello world", cursor(0, 6), "vb");
neovim_test!(visual, v_select_gg, "line1\nline2\nline3", cursor(2, 0), "vgg");
neovim_test!(visual, v_select_G, "line1\nline2\nline3", "vG");
neovim_test!(visual, v_select_unicode, "日本語 hello", "vw");
neovim_test!(visual, v_select_multiline, "hello\nworld", "vj");
neovim_test!(visual, v_select_count, "hello world test", "v3l");
neovim_test!(visual, v_select_percent, "(hello)", "v%");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL LINE SELECTION
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual, V_select_down, "line1\nline2\nline3", "Vj");
neovim_test!(visual, V_select_multiple, "l1\nl2\nl3\nl4", "V2j");

// NEW: More line selection edge cases
neovim_test!(visual, V_select_up, "line1\nline2\nline3", cursor(2, 0), "Vk");
neovim_test!(visual, V_select_multiple_up, "l1\nl2\nl3\nl4", cursor(3, 0), "V2k");
neovim_test!(visual, V_select_gg, "line1\nline2\nline3", cursor(2, 0), "Vgg");
neovim_test!(visual, V_select_G, "line1\nline2\nline3", "VG");
neovim_test!(visual, V_single_line_eol, "hello", cursor(0, 4), "V");
neovim_test!(visual, V_last_line, "line1\nline2", cursor(1, 0), "V");
neovim_test!(visual, V_first_line, "line1\nline2", "V");
neovim_test!(visual, V_all_lines, "l1\nl2\nl3", "VG");
neovim_test!(visual, V_unicode, "日本語\nテスト", "Vj");
neovim_test!(visual, V_with_indent, "  hello\n  world", "Vj");
neovim_test!(visual, V_count, "l1\nl2\nl3\nl4", "V3j");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL BLOCK SELECTION
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual, ctrl_v_column, "ab\ncd", "<C-v>j");
neovim_test!(visual, ctrl_v_rectangle, "abc\ndef\nghi", "<C-v>jl");

// NEW: More block selection edge cases
neovim_test!(visual, ctrl_v_column_down, "abc\ndef\nghi", "<C-v>2j");
neovim_test!(visual, ctrl_v_column_right, "abc\ndef\nghi", "<C-v>2l");
neovim_test!(visual, ctrl_v_full_rectangle, "abc\ndef\nghi", "<C-v>2j2l");
neovim_test!(visual, ctrl_v_uneven_lines, "abcdef\nab\nabcd", "<C-v>2j2l");
neovim_test!(visual, ctrl_v_single_column, "abc\ndef", "<C-v>j");
neovim_test!(visual, ctrl_v_unicode, "日本語\nテスト", "<C-v>j");
neovim_test!(visual, ctrl_v_dollar, "hello\nhi\nhello_world", "<C-v>j$");
neovim_test!(visual, ctrl_v_at_eol, "abc\ndefghij", cursor(0, 2), "<C-v>j");
neovim_test!(visual, ctrl_v_backward, "abc\ndef", cursor(0, 2), "<C-v>jh");
neovim_test!(visual, ctrl_v_upward, "abc\ndef\nghi", cursor(2, 0), "<C-v>2k");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL MODE OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual, v_delete, "hello world", "vwd");
neovim_test!(visual, v_yank, "hello world", "vwy");
neovim_test!(visual, v_change, "hello world", "vwcX<Esc>");
neovim_test!(visual, V_delete, "line1\nline2\nline3", "Vjd");
neovim_test!(visual, V_yank, "line1\nline2", "Vy");

// NEW: More operation edge cases
neovim_test!(visual, v_delete_word, "hello world", "vewd");
neovim_test!(visual, v_delete_line, "hello world", "v$d");
neovim_test!(visual, v_yank_paste, "hello world", "vwyp");
neovim_test!(visual, v_change_word, "hello world", "vwcbye<Esc>");
neovim_test!(visual, V_delete_all, "line1\nline2", "VGd");
neovim_test!(visual, V_yank_paste, "line1\nline2", "Vyjp");
neovim_test!(visual, V_change, "hello", "VcX<Esc>");
neovim_test!(visual, ctrl_v_delete, "abc\ndef\nghi", "<C-v>jld");
neovim_test!(visual, ctrl_v_yank, "abc\ndef\nghi", "<C-v>jly");
neovim_test!(visual, ctrl_v_change, "abc\ndef\nghi", "<C-v>jlcX<Esc>");
neovim_test!(visual, v_uppercase, "hello", "vwgU");
neovim_test!(visual, v_lowercase, "HELLO", "vwgu");
neovim_test!(visual, v_toggle_case, "HeLLo", "vwg~");
neovim_test!(visual, V_indent, "hello\nworld", "Vj>");
neovim_test!(visual, V_outdent, "    hello\n    world", "Vj<");
neovim_test!(visual, v_join, "hello\nworld", "Vj:j<CR>");
neovim_test!(visual, v_sort, "c\nb\na", "VGg:sort<CR>");
neovim_test!(visual, v_delete_unicode, "日本語 hello", "vwd");
neovim_test!(visual, V_delete_unicode, "日本語\nテスト", "Vjd");
neovim_test!(visual, v_yank_register, "hello world", "vw\"ay");
neovim_test!(visual, v_delete_register, "hello world", "vw\"ad");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL MODE EXIT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual, v_escape, "hello", "v<Esc>");
neovim_test!(visual, v_toggle_to_V, "hello", "vV");
neovim_test!(visual, V_toggle_to_v, "hello", "Vv");
neovim_test!(visual, v_toggle_to_block, "hello", "v<C-v>");

// NEW: More exit edge cases
neovim_test!(visual, V_escape, "hello", "V<Esc>");
neovim_test!(visual, ctrl_v_escape, "hello", "<C-v><Esc>");
neovim_test!(visual, V_toggle_to_block, "hello", "V<C-v>");
neovim_test!(visual, ctrl_v_toggle_to_v, "hello", "<C-v>v");
neovim_test!(visual, ctrl_v_toggle_to_V, "hello", "<C-v>V");
neovim_test!(visual, v_double_v_exit, "hello", "vv");
neovim_test!(visual, V_double_V_exit, "hello", "VV");
neovim_test!(visual, ctrl_v_double_exit, "hello", "<C-v><C-v>");
neovim_test!(visual, v_escape_cursor_pos, "hello", cursor(0, 2), "vl<Esc>");
neovim_test!(visual, V_escape_cursor_pos, "line1\nline2", "Vj<Esc>");
neovim_test!(visual, v_ctrl_c, "hello", "vl<C-c>");

// ═══════════════════════════════════════════════════════════════════════════════
// SWAP ANCHOR/CURSOR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual, o_swap_anchor, "hello", "vllo");
neovim_test!(visual, O_swap_block_corner, "ab\ncd", "<C-v>jlO");

// NEW: More swap edge cases
neovim_test!(visual, o_swap_back, "hello world", "vwoh");
neovim_test!(visual, o_swap_forward, "hello world", cursor(0, 10), "v0ol");
neovim_test!(visual, V_o_swap, "line1\nline2\nline3", "Vjo");
neovim_test!(visual, o_double_swap, "hello world", "vwoo");
neovim_test!(visual, O_block_swap, "abc\ndef\nghi", "<C-v>jlOO");
neovim_test!(visual, o_then_extend, "hello world test", "vwowl");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL WITH TEXT OBJECTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual, v_inner_word, "hello world", "viw");
neovim_test!(visual, v_a_word, "hello world", "vaw");
neovim_test!(visual, v_inner_quotes, "\"hello\" world", cursor(0, 1), "vi\"");
neovim_test!(visual, v_inner_parens, "(hello) world", cursor(0, 1), "vi(");

// NEW: More text object edge cases
neovim_test!(visual, v_inner_WORD, "foo.bar baz", "viW");
neovim_test!(visual, v_a_WORD, "foo.bar baz", "vaW");
neovim_test!(visual, v_inner_braces, "{hello}", cursor(0, 1), "vi{");
neovim_test!(visual, v_a_braces, "{hello}", cursor(0, 1), "va{");
neovim_test!(visual, v_inner_brackets, "[hello]", cursor(0, 1), "vi[");
neovim_test!(visual, v_a_brackets, "[hello]", cursor(0, 1), "va[");
neovim_test!(visual, v_inner_angle, "<hello>", cursor(0, 1), "vi<");
neovim_test!(visual, v_a_angle, "<hello>", cursor(0, 1), "va<");
neovim_test!(visual, v_inner_single_quote, "'hello'", cursor(0, 1), "vi'");
neovim_test!(visual, v_a_single_quote, "'hello'", cursor(0, 1), "va'");
neovim_test!(visual, v_inner_backtick, "`hello`", cursor(0, 1), "vi`");
neovim_test!(visual, v_a_backtick, "`hello`", cursor(0, 1), "va`");
neovim_test!(visual, v_inner_paragraph, "para1\n\npara2", "vip");
neovim_test!(visual, v_a_paragraph, "para1\n\npara2", "vap");
neovim_test!(visual, v_inner_sentence, "Hello. World.", "vis");
neovim_test!(visual, v_a_sentence, "Hello. World.", "vas");
neovim_test!(visual, v_inner_tag, "<div>hello</div>", cursor(0, 5), "vit");
neovim_test!(visual, v_a_tag, "<div>hello</div>", cursor(0, 5), "vat");
neovim_test!(visual, v_extend_iw, "one two three", "viwiwl");
neovim_test!(visual, v_extend_aw, "one two three", "vawawl");
neovim_test!(visual, v_textobj_then_delete, "hello world", "viwd");
neovim_test!(visual, v_textobj_then_yank, "hello world", "viwy");
neovim_test!(visual, v_textobj_then_change, "hello world", "viwcX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// RESELECT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual, gv_reselect, "hello world", "vw<Esc>gv");

// NEW: More reselect edge cases
neovim_test!(visual, gv_reselect_line, "line1\nline2", "Vj<Esc>gv");
neovim_test!(visual, gv_reselect_block, "abc\ndef", "<C-v>jl<Esc>gv");
neovim_test!(visual, gv_after_delete, "hello world", "vwd0gv");
neovim_test!(visual, gv_after_yank, "hello world", "vwy$gv");
neovim_test!(visual, gv_after_change, "hello world", "vwcX<Esc>gv");
neovim_test!(visual, gv_multiline, "line1\nline2\nline3", "Vjj<Esc>gv");
neovim_test!(visual, gv_unicode, "日本語 テスト", "vw<Esc>gv");

// Undo restores visual area: gv after undo shows the original visual selection
neovim_test!(visual, gv_after_undo, "hello world", "vwdugv");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL BLOCK INSERT/APPEND
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual, ctrl_v_I_insert, "abc\ndef\nghi", "<C-v>2jIX<Esc>");
neovim_test!(visual, ctrl_v_A_append, "abc\ndef\nghi", "<C-v>2jlAX<Esc>");
neovim_test!(visual, ctrl_v_c_change, "abc\ndef\nghi", "<C-v>2jcX<Esc>");
neovim_test!(visual, ctrl_v_I_middle, "abc\ndef\nghi", cursor(0, 1), "<C-v>2jIX<Esc>");
neovim_test!(visual, ctrl_v_A_eol, "abc\ndef\nghi", "<C-v>2j$AX<Esc>");
neovim_test!(visual, ctrl_v_I_unicode, "日本語\nテスト\nアイウ", "<C-v>2jIX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL MODE WITH COUNTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual, v_count_motion, "hello world test", "v2w");
neovim_test!(visual, v_count_l, "hello world", "v5l");
neovim_test!(visual, v_count_h, "hello world", cursor(0, 10), "v5h");
neovim_test!(visual, V_count_j, "l1\nl2\nl3\nl4", "V3j");
neovim_test!(visual, V_count_k, "l1\nl2\nl3\nl4", cursor(3, 0), "V3k");
neovim_test!(visual, ctrl_v_count, "abc\ndef\nghi", "<C-v>2j2l");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL MODE MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual, v_motion_w, "one two three", "vw");
neovim_test!(visual, v_motion_e, "one two three", "ve");
neovim_test!(visual, v_motion_b, "one two three", cursor(0, 8), "vb");
neovim_test!(visual, v_motion_0, "hello world", cursor(0, 6), "v0");
neovim_test!(visual, v_motion_dollar, "hello world", "v$");
neovim_test!(visual, v_motion_caret, "  hello world", cursor(0, 8), "v^");
neovim_test!(visual, v_motion_f, "hello world", "vfo");
neovim_test!(visual, v_motion_t, "hello world", "vto");
neovim_test!(visual, v_motion_F, "hello world", cursor(0, 10), "vFo");
neovim_test!(visual, v_motion_T, "hello world", cursor(0, 10), "vTo");
neovim_test!(visual, v_motion_percent, "(hello)", "v%");
neovim_test!(visual, v_motion_paragraph, "para1\n\npara2", "v}");
neovim_test!(visual, v_motion_sentence, "Hello. World.", "v)");
neovim_test!(visual, v_motion_gg, "l1\nl2\nl3", cursor(2, 0), "vgg");
neovim_test!(visual, v_motion_G, "l1\nl2\nl3", "vG");
neovim_test!(visual, v_motion_H, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(5, 0), "vH");
neovim_test!(visual, v_motion_L, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(0, 0), "vL");
neovim_test!(visual, v_motion_M, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "vM");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES - COMPREHENSIVE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual, v_at_eol, "hello", cursor(0, 4), "vl");
neovim_test!(visual, v_backwards, "hello", cursor(0, 4), "vhh");

// NEW: More edge cases
neovim_test!(visual, v_at_bol, "hello", "vh");
neovim_test!(visual, v_single_char_buffer, "a", "v");
neovim_test!(visual, V_single_line_buffer, "hello", "V");
neovim_test!(visual, ctrl_v_single_cell, "a", "<C-v>");
neovim_test!(visual, v_entire_line, "hello", "v$");
neovim_test!(visual, v_entire_buffer, "hello", "vG$");
neovim_test!(visual, V_entire_buffer, "l1\nl2\nl3", "VG");
neovim_test!(visual, ctrl_v_entire_column, "a\nb\nc", "<C-v>G");
neovim_test!(visual, v_long_line, "abcdefghijklmnopqrstuvwxyz", "v$");
neovim_test!(visual, V_many_lines, "l1\nl2\nl3\nl4\nl5", "VG");
neovim_test!(visual, v_emoji, "👍 test", "vw");
neovim_test!(visual, v_combining_chars, "e\u{0301}llo", "vw");
neovim_test!(visual, v_cjk, "日本語", "v$");
neovim_test!(visual, V_cjk, "日本語\nテスト", "Vj");
neovim_test!(visual, ctrl_v_mixed_width, "abc日本語\ndef日本語", "<C-v>jl");
neovim_test!(visual, v_across_blank_lines, "para1\n\npara2", "vG");
neovim_test!(visual, v_tab_chars, "a\tb\tc", "v$");
neovim_test!(visual, v_whitespace_only, "   ", "v$");
neovim_test!(visual, v_then_dot, "hello world", "vwdw.");
neovim_test!(visual, v_search, "hello world hello", "v/hello<CR>");

// ─────────────────────────────────────────────────────────────────────────────
// Visual Mode Boundary Edge Cases
// ─────────────────────────────────────────────────────────────────────────────

// Visual extend past end of line
neovim_test!(visual, v_extend_past_eol, "hello", "v$d");
// Visual line at last line
neovim_test!(visual, V_at_last_line, "aaa\nbbb\nccc", cursor(2, 0), "Vd");
// Visual line at first line
neovim_test!(visual, V_at_first_line, "aaa\nbbb\nccc", "Vd");
// V select entire buffer
neovim_test!(visual, V_entire_one_line, "hello", "Vd");
// Visual line + D (uppercase) — deletes selected lines
neovim_test!(visual, V_delete_D, "aaa\nbbb\nccc", "VD");
neovim_test!(visual, V_delete_D_multiline, "aaa\nbbb\nccc\nddd", "VjD");
neovim_test!(visual, V_delete_D_last_line, "aaa\nbbb\nccc", cursor(2, 0), "VD");
neovim_test!(visual, V_delete_D_middle, "aaa\nbbb\nccc", cursor(1, 0), "VD");
// Visual block with uneven lines
neovim_test!(visual, vblock_uneven_lines, "long line here\nshort\nanother long one", "<C-v>2j10ld");
// Visual on whitespace-only line
neovim_test!(visual, v_whitespace_line, "   ", "vld");
// Visual backward past anchor with o
neovim_test!(visual, v_backward_past_start, "hello world test", "wvwohd");
// gv on empty (no previous visual)
neovim_test!(visual, gv_no_previous, "hello", "gv");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL BLOCK — EXPANDED EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(visual, vblock_replace_single_col, "aaa\nbbb\nccc", "<C-v>jjrx");
neovim_test!(visual, vblock_tilde, "aaa\nbbb\nccc", "<C-v>jj~");
neovim_test!(visual, vblock_indent, "aaa\nbbb\nccc", "<C-v>jj>");
neovim_test!(visual, vblock_outdent, "    aaa\n    bbb\n    ccc", "<C-v>jj<");
neovim_test!(visual, vblock_dollar, "short\nlonger text\nhi", "<C-v>j$d");
neovim_test!(visual, vblock_short_lines, "abcde\nab\nabcde", "<C-v>jjlld");
neovim_test!(visual, vblock_gv_after_op, "aaa\nbbb\nccc", "<C-v>jjd gv");

// ═══════════════════════════════════════════════════════════════════════════════
// gv EXCHANGE (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// gv from normal restores previous visual selection
neovim_test!(visual, nf_gv_restore, "hello world", "vw<Esc>gv");

// gv in visual swaps to previous selection (on different line)
neovim_test!(visual, nf_gv_swap, "hello\nworld", "vl<Esc>jvlgv");

// ═══════════════════════════════════════════════════════════════════════════════
// BLOCK VISUAL $ (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// Ctrl-V $ j — block visual extends to EOL on each line
neovim_test!(visual, nf_block_dollar_j, "abc\nde\nfghij", "<C-v>$j");
