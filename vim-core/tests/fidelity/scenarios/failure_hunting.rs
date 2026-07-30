// Failure hunting fidelity tests.
//
// Targeted edge cases designed to expose bugs in cursor positioning,
// count handling, dot repeat interactions, boundary conditions, and
// multi-step command sequences. Organized by failure category.

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR POSITION AFTER LENGTH-CHANGING OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════
// The ctrl-a/ctrl-x bug showed cursor lands wrong when text length changes.
// These tests probe the same class of bug in other operations.

// Repeated paste: each paste shifts text, cursor must track correctly
neovim_test!(scenarios, fh_repeated_p_charwise, "hello", "ywp..");
neovim_test!(scenarios, fh_repeated_p_linewise, "hello\nworld", "yy3p");
neovim_test!(scenarios, fh_repeated_P_charwise, "hello", "ywP..");
neovim_test!(scenarios, fh_repeated_dd_then_p, "a\nb\nc\nd\ne", "ddjjp");

// Repeated x at end of line — cursor must retreat each time
neovim_test!(scenarios, fh_repeated_x_at_eol, "hello", "$xxxx");
neovim_test!(scenarios, fh_repeated_x_single_char_lines, "a\nb\nc", "xjxjx");

// Repeated J (join) — each join shortens line count, cursor must track
neovim_test!(scenarios, fh_repeated_J_three_lines, "a\nb\nc\nd", "JJJ");
neovim_test!(scenarios, fh_repeated_gJ_three_lines, "a\nb\nc\nd", "gJgJgJ");

// Repeated r (replace) — cursor must stay on replaced char
neovim_test!(scenarios, fh_repeated_r_across_word, "hello", "rxlrxlrx");

// Delete word repeatedly — text shrinks, cursor stays at deletion point
neovim_test!(scenarios, fh_repeated_dw, "one two three four five", "dw...");
neovim_test!(scenarios, fh_repeated_de, "one two three four five", "de...");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT COUNT OVERRIDE
// ═══════════════════════════════════════════════════════════════════════════════
// In Vim, if you do 3dd then 5., the . does 5dd (overrides the original count).
// If you do 3dd then ., the . does 3dd (preserves original count).

neovim_test!(scenarios, fh_dot_count_override_dd, "a\nb\nc\nd\ne\nf\ng\nh\ni\nj", "2ddj5.");
neovim_test!(scenarios, fh_dot_count_override_dw, "one two three four five six seven", "2dw3.");
neovim_test!(scenarios, fh_dot_count_override_x, "abcdefghij", "3x2.");
neovim_test!(scenarios, fh_dot_no_override_dd, "a\nb\nc\nd\ne\nf\ng", "3dd.");
neovim_test!(scenarios, fh_dot_count_override_cw, "aaa bbb ccc ddd eee fff", "cwXXX<Esc>w2.");
neovim_test!(scenarios, fh_dot_count_override_rx, "abcdefgh", "3rx2.");

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT MODE WITH COUNTS
// ═══════════════════════════════════════════════════════════════════════════════
// 3iabc<Esc> should insert "abcabcabc". This is a common bug area.

neovim_test!(scenarios, fh_insert_with_count_2, "hello", "2iX<Esc>");
neovim_test!(scenarios, fh_insert_with_count_3, "hello", "3iabc<Esc>");
neovim_test!(scenarios, fh_insert_with_count_5, "", "5ix<Esc>");
neovim_test!(scenarios, fh_append_with_count_3, "hello", "3aX<Esc>");
neovim_test!(scenarios, fh_open_with_count_3, "hello", "3oX<Esc>");
neovim_test!(scenarios, fh_Open_with_count_2, "hello", "2OX<Esc>");
neovim_test!(scenarios, fh_insert_count_multichar, "test", "3iab<Esc>");
neovim_test!(scenarios, fh_insert_count_with_newline, "test", "2ia<CR>b<Esc>");
neovim_test!(scenarios, fh_insert_count_at_eol, "hello", "$3ax<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR POSITION AT DOCUMENT BOUNDARIES
// ═══════════════════════════════════════════════════════════════════════════════

// dd on last line — cursor should go to new last line
neovim_test!(scenarios, fh_dd_last_line, "a\nb\nc", cursor(2, 0), "dd");
neovim_test!(scenarios, fh_dd_only_line, "hello", "dd");
neovim_test!(scenarios, fh_dd_second_to_last, "a\nb\nc", cursor(1, 0), "dd");

// dG from various positions
neovim_test!(scenarios, fh_dG_from_first, "a\nb\nc\nd", "dG");
neovim_test!(scenarios, fh_dG_from_mid, "a\nb\nc\nd", cursor(1, 0), "dG");
neovim_test!(scenarios, fh_dG_from_last, "a\nb\nc\nd", cursor(3, 0), "dG");

// dgg from various positions
neovim_test!(scenarios, fh_dgg_from_last, "a\nb\nc\nd", cursor(3, 0), "dgg");
neovim_test!(scenarios, fh_dgg_from_mid, "a\nb\nc\nd", cursor(2, 0), "dgg");

// D on empty line
neovim_test!(scenarios, fh_D_empty_line, "hello\n\nworld", cursor(1, 0), "D");
neovim_test!(scenarios, fh_C_empty_line, "hello\n\nworld", cursor(1, 0), "Ctest<Esc>");
neovim_test!(scenarios, fh_S_empty_line, "\n", "Shello<Esc>");
neovim_test!(scenarios, fh_cc_empty_line, "hello\n\nworld", cursor(1, 0), "cctest<Esc>");

// x on single char document
neovim_test!(scenarios, fh_x_single_char_doc, "a", "x");
neovim_test!(scenarios, fh_X_at_bol, "hello", "X");
neovim_test!(scenarios, fh_X_at_col1, "hello", "lX");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATIONS ON EMPTY LINES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fh_dw_on_empty_line, "hello\n\nworld", cursor(1, 0), "dw");
neovim_test!(scenarios, fh_cw_on_empty_line, "hello\n\nworld", cursor(1, 0), "cwtest<Esc>");
neovim_test!(scenarios, fh_yw_on_empty_line, "hello\n\nworld", cursor(1, 0), "ywp");
neovim_test!(scenarios, fh_yy_on_empty_line, "hello\n\nworld", cursor(1, 0), "yyp");
neovim_test!(scenarios, fh_p_after_yy_empty, "hello\n\nworld", cursor(1, 0), "yyjp");
neovim_test!(scenarios, fh_J_on_empty_line, "hello\n\nworld", cursor(1, 0), "J");
neovim_test!(scenarios, fh_tilde_on_empty_line, "hello\n\nworld", cursor(1, 0), "~");

// ═══════════════════════════════════════════════════════════════════════════════
// CW vs CE DISTINCTION
// ═══════════════════════════════════════════════════════════════════════════════
// cw at end of word does NOT include trailing whitespace (unlike dw).
// This is a documented vim quirk.

neovim_test!(scenarios, fh_cw_mid_word, "hello world", "cwbye<Esc>");
neovim_test!(scenarios, fh_cw_start_word, "hello world", cursor(0, 6), "cwbye<Esc>");
neovim_test!(scenarios, fh_cw_at_eol, "hello world", cursor(0, 6), "cwtest<Esc>");
neovim_test!(scenarios, fh_dw_vs_cw_trailing_space, "hello   world", "dw");
neovim_test!(scenarios, fh_cw_vs_ce_same, "hello world", "cwX<Esc>");
neovim_test!(scenarios, fh_dw_end_of_line, "hello world\nnext", cursor(0, 6), "dw");
neovim_test!(scenarios, fh_cw_end_of_line, "hello world\nnext", cursor(0, 6), "cwX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// PASTE EDGE CASES (CURSOR POSITIONING)
// ═══════════════════════════════════════════════════════════════════════════════

// p after yy — linewise paste below, cursor at first non-blank
neovim_test!(scenarios, fh_p_linewise_indented, "  hello\n  world", "yyp");
neovim_test!(scenarios, fh_P_linewise_indented, "  hello\n  world", "yyP");

// p on last line of doc with linewise content
neovim_test!(scenarios, fh_p_linewise_on_last_line, "a\nb\nc", cursor(2, 0), "yyp");
neovim_test!(scenarios, fh_P_linewise_on_first_line, "a\nb\nc", "yyP");

// p/P with empty register (no-op)
neovim_test!(scenarios, fh_p_empty_register, "hello", "\"zp");
neovim_test!(scenarios, fh_P_empty_register, "hello", "\"zP");

// Charwise paste of text containing newline
neovim_test!(scenarios, fh_p_charwise_with_newline, "hello world", "wv$hy0p");
neovim_test!(scenarios, fh_yank_dollar_paste, "hello world\nfoo", "y$jp");

// xp = transpose two characters
neovim_test!(scenarios, fh_xp_transpose, "abcde", "xp");
neovim_test!(scenarios, fh_xp_transpose_mid, "abcde", cursor(0, 2), "xp");
neovim_test!(scenarios, fh_xp_at_eol, "abcde", cursor(0, 3), "xp");

// ddp = swap two lines
neovim_test!(scenarios, fh_ddp_swap_lines, "aaa\nbbb\nccc", "ddp");
neovim_test!(scenarios, fh_ddp_swap_at_end, "aaa\nbbb\nccc", cursor(1, 0), "ddp");

// Paste with count
neovim_test!(scenarios, fh_p_with_count_3, "hello", "yw3p");
neovim_test!(scenarios, fh_p_linewise_count_3, "hello\nworld", "yy3p");

// ═══════════════════════════════════════════════════════════════════════════════
// REGISTER INTERACTIONS
// ═══════════════════════════════════════════════════════════════════════════════

// Numbered register rotation: dd pushes to "1, previous "1 goes to "2, etc.
neovim_test!(scenarios, fh_numbered_register_rotation, "a\nb\nc\nd", "ddjdd\"1p");
neovim_test!(scenarios, fh_numbered_register_2, "a\nb\nc\nd\ne", "ddjddjdd\"2p");
neovim_test!(scenarios, fh_small_delete_register, "hello world", "dw\"-p");

// Append register: "ayy then "Ayy appends
neovim_test!(scenarios, fh_append_register, "hello\nworld\nfoo", "\"ayyjj\"Ayy\"ap");
neovim_test!(scenarios, fh_append_register_charwise, "hello world", "\"ayww\"Aywj\"ap");

// Named register survives other operations
neovim_test!(scenarios, fh_named_register_survives_dd, "hello\nworld\nfoo", "\"ayyjddj\"ap");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO GRANULARITY IN INSERT MODE
// ═══════════════════════════════════════════════════════════════════════════════
// Vim treats one insert session as one undo group.
// Cursor keys or certain control chars break the undo group.

neovim_test!(scenarios, fh_undo_whole_insert, "hello", "Aworld<Esc>u");
neovim_test!(scenarios, fh_undo_insert_then_normal_edit, "hello", "Aworld<Esc>xpau");
neovim_test!(scenarios, fh_undo_two_inserts, "hello", "A one<Esc>A two<Esc>u");
neovim_test!(scenarios, fh_undo_two_inserts_twice, "hello", "A one<Esc>A two<Esc>uu");
neovim_test!(scenarios, fh_redo_after_undo_insert, "hello", "A world<Esc>u<C-r>");
neovim_test!(scenarios, fh_undo_o_insert, "hello", "oworld<Esc>u");
neovim_test!(scenarios, fh_undo_O_insert, "hello", "Oworld<Esc>u");
neovim_test!(scenarios, fh_undo_cw_insert, "hello world", "cwXXX<Esc>u");
neovim_test!(scenarios, fh_undo_s_insert, "hello", "sX<Esc>u");
neovim_test!(scenarios, fh_undo_count_insert, "hello", "3ix<Esc>u");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT OF COMPLEX OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

// dot after r (replace char)
neovim_test!(scenarios, fh_dot_after_r, "abcdef", "rxl.");
neovim_test!(scenarios, fh_dot_after_r_mid, "abcdef", cursor(0, 2), "rxll.");

// dot after ~ (toggle case)
neovim_test!(scenarios, fh_dot_after_tilde, "hello", "~.");
neovim_test!(scenarios, fh_dot_after_3tilde, "hello world", "3~.");

// dot after J (join)
neovim_test!(scenarios, fh_dot_after_J, "a\nb\nc\nd", "J.");
neovim_test!(scenarios, fh_dot_after_gJ, "a\nb\nc\nd", "gJ.");

// dot after << and >>
neovim_test!(scenarios, fh_dot_after_indent, "  hello\n  world", ">>.j.");
neovim_test!(scenarios, fh_dot_after_outdent, "    hello\n    world", "<<j.");

// dot after visual operation
neovim_test!(scenarios, fh_dot_after_visual_delete, "hello world foo", "vwd.");
neovim_test!(scenarios, fh_dot_after_visual_case, "hello world foo", "vwU.");

// dot after c with motion
neovim_test!(scenarios, fh_dot_after_cf, "hello-world-test", "cf-X<Esc>.");
neovim_test!(scenarios, fh_dot_after_ct, "hello-world-test", "ct-X<Esc>w.");

// dot after s
neovim_test!(scenarios, fh_dot_after_s, "hello", "sX<Esc>l.");
neovim_test!(scenarios, fh_dot_after_3s, "hello world", "3sX<Esc>w.");

// dot after visual line delete
neovim_test!(scenarios, fh_dot_after_Vd, "a\nb\nc\nd\ne", "Vd.");

// ═══════════════════════════════════════════════════════════════════════════════
// MOTION EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

// f/F/t/T on same character that cursor is on
neovim_test!(scenarios, fh_f_on_current_char, "aabaa", "fa");
neovim_test!(scenarios, fh_f_no_match, "hello", "fx");
neovim_test!(scenarios, fh_t_on_adjacent, "abc", "tb");
neovim_test!(scenarios, fh_semicolon_no_cross_line, "a.b\nc.d", "f.j;");
neovim_test!(scenarios, fh_comma_reverse_find, "a.b.c", "f.;,");

// w/b/e across empty lines
neovim_test!(scenarios, fh_w_across_empty_lines, "hello\n\n\nworld", "www");
neovim_test!(scenarios, fh_b_across_empty_lines, "hello\n\n\nworld", cursor(3, 0), "bbb");
neovim_test!(scenarios, fh_e_across_empty_lines, "hello\n\n\nworld", "eee");

// W/B/E (WORD motions) vs w/b/e (word motions)
neovim_test!(scenarios, fh_W_with_punctuation, "hello.world foo", "W");
neovim_test!(scenarios, fh_w_with_punctuation, "hello.world foo", "w");
neovim_test!(scenarios, fh_E_with_punctuation, "hello.world foo", "E");
neovim_test!(scenarios, fh_e_with_punctuation, "hello.world foo", "e");
neovim_test!(scenarios, fh_B_with_punctuation, "hello.world foo", cursor(0, 12), "B");
neovim_test!(scenarios, fh_b_with_punctuation, "hello.world foo", cursor(0, 12), "b");

// % on various brackets
neovim_test!(scenarios, fh_percent_paren, "(hello)", "%");
neovim_test!(scenarios, fh_percent_bracket, "[hello]", "%");
neovim_test!(scenarios, fh_percent_brace, "{hello}", "%");
neovim_test!(scenarios, fh_percent_nested, "((a)(b))", "%");
neovim_test!(scenarios, fh_percent_no_match, "hello", "%");
neovim_test!(scenarios, fh_percent_from_inside, "(hello)", cursor(0, 3), "%");

// gg and G with counts
neovim_test!(scenarios, fh_gg_with_count, "a\nb\nc\nd\ne", "3gg");
neovim_test!(scenarios, fh_G_with_count, "a\nb\nc\nd\ne", "3G");
neovim_test!(scenarios, fh_G_beyond_last_line, "a\nb\nc", "99G");

// ═══════════════════════════════════════════════════════════════════════════════
// TEXT OBJECTS AT BOUNDARIES
// ═══════════════════════════════════════════════════════════════════════════════

// ci" when cursor is NOT inside quotes
neovim_test!(scenarios, fh_ci_dquote_outside, "hello \"world\" test", "ci\"X<Esc>");
neovim_test!(scenarios, fh_ci_dquote_on_quote, "hello \"world\" test", cursor(0, 6), "ci\"X<Esc>");
neovim_test!(scenarios, fh_ci_dquote_after_quotes, "hello \"world\" test", cursor(0, 14), "ci\"X<Esc>");

// di( when cursor is outside parens
neovim_test!(scenarios, fh_di_paren_outside, "hello (world) test", "di(");
neovim_test!(scenarios, fh_di_paren_on_paren, "hello (world) test", cursor(0, 6), "di(");

// Text objects on empty content
neovim_test!(scenarios, fh_di_paren_empty, "()", "ldi(");
neovim_test!(scenarios, fh_ci_paren_empty, "()", "lci(X<Esc>");
neovim_test!(scenarios, fh_di_dquote_empty, "\"\"", "ldi\"");
neovim_test!(scenarios, fh_di_brace_empty, "{}", "ldi{");

// diw / daw at start and end of line
neovim_test!(scenarios, fh_diw_at_start, "hello world", "diw");
neovim_test!(scenarios, fh_daw_at_start, "hello world", "daw");
neovim_test!(scenarios, fh_diw_at_end, "hello world", cursor(0, 6), "diw");
neovim_test!(scenarios, fh_daw_at_end, "hello world", cursor(0, 6), "daw");
neovim_test!(scenarios, fh_diw_single_word, "hello", "diw");
neovim_test!(scenarios, fh_daw_single_word, "hello", "daw");

// dis / das (sentence objects)
neovim_test!(scenarios, fh_dis_basic, "Hello world. Foo bar. Baz.", cursor(0, 13), "dis");
neovim_test!(scenarios, fh_das_basic, "Hello world. Foo bar. Baz.", cursor(0, 13), "das");
neovim_test!(scenarios, fh_dis_at_start, "Hello world. Foo bar.", "dis");
neovim_test!(scenarios, fh_das_at_end, "Hello world. Foo bar.", cursor(0, 13), "das");

// dip / dap (paragraph objects)
neovim_test!(scenarios, fh_dip_single_para, "hello\nworld\n\nfoo", "dip");
neovim_test!(scenarios, fh_dap_single_para, "hello\nworld\n\nfoo", "dap");
neovim_test!(scenarios, fh_dip_at_blank_line, "hello\n\nworld", cursor(1, 0), "dip");
neovim_test!(scenarios, fh_dap_last_para, "hello\n\nworld\nfoo", cursor(2, 0), "dap");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL MODE CURSOR AND SELECTION EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

// Visual mode o (swap anchor/cursor) then operation
neovim_test!(scenarios, fh_visual_o_then_delete, "hello world", "vwod");
neovim_test!(scenarios, fh_visual_o_then_yank, "hello world", "v$oy0p");
neovim_test!(scenarios, fh_visual_line_o, "a\nb\nc\nd", "Vjod");

// gv (reselect last visual)
neovim_test!(scenarios, fh_gv_after_delete, "hello world test", "vwdgvd");
neovim_test!(scenarios, fh_gv_after_yank, "hello world test", "vwyugvd");
neovim_test!(scenarios, fh_gv_line_mode, "a\nb\nc\nd", "Vjdgvd");

// Visual mode with counts
neovim_test!(scenarios, fh_visual_3w, "one two three four five", "v3wd");
neovim_test!(scenarios, fh_visual_3j, "a\nb\nc\nd\ne", "v3jd");

// ═══════════════════════════════════════════════════════════════════════════════
// CHANGE LIST NAVIGATION (g; and g,)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fh_g_semicolon_basic, "hello\nworld\nfoo", "A!<Esc>jA!<Esc>g;");
neovim_test!(scenarios, fh_g_semicolon_twice, "hello\nworld\nfoo", "A!<Esc>jA!<Esc>jA!<Esc>g;g;");
neovim_test!(scenarios, fh_g_comma_after_g_semicolon, "hello\nworld\nfoo", "A!<Esc>jA!<Esc>g;g,");
neovim_test!(scenarios, fh_g_semicolon_no_changes, "hello", "g;");
neovim_test!(scenarios, fh_g_semicolon_after_dd, "hello\nworld\nfoo", cursor(1, 0), "ddjg;");

// ═══════════════════════════════════════════════════════════════════════════════
// JUMP LIST (Ctrl-O / Ctrl-I)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fh_ctrl_o_after_gg, "a\nb\nc\nd\ne\nf\ng\nh\ni\nj", cursor(9, 0), "gg<C-o>");
neovim_test!(scenarios, fh_ctrl_o_after_G, "a\nb\nc\nd\ne\nf\ng\nh\ni\nj", "G<C-o>");
neovim_test!(scenarios, fh_ctrl_o_after_search, "hello\nworld\nhello", "/hello<CR><C-o>");
neovim_test!(scenarios, fh_ctrl_i_after_ctrl_o, "a\nb\nc\nd\ne", "G<C-o><C-i>");
neovim_test!(scenarios, fh_ctrl_o_multiple, "a\nb\nc\nd\ne", "Ggg$<C-o><C-o>");
neovim_test!(scenarios, fh_ctrl_o_after_percent, "(hello) (world)", "%(hello) (world)");

// ═══════════════════════════════════════════════════════════════════════════════
// MARKS AFTER TEXT MUTATIONS
// ═══════════════════════════════════════════════════════════════════════════════

// Mark position should update when text is inserted/deleted before it
neovim_test!(scenarios, fh_mark_after_insert_before, "hello world", cursor(0, 6), "maw0iXXX <Esc>`a");
neovim_test!(scenarios, fh_mark_after_delete_before, "hello world test", cursor(0, 12), "maw0dw`a");
neovim_test!(scenarios, fh_mark_after_dd_before, "aaa\nbbb\nccc", cursor(2, 0), "maw0dd`a");
neovim_test!(scenarios, fh_mark_after_o_before, "hello\nworld", cursor(1, 0), "mawkohi<Esc>`a");

// ═══════════════════════════════════════════════════════════════════════════════
// SPECIAL REPLACE MODE SCENARIOS
// ═══════════════════════════════════════════════════════════════════════════════

// Replace mode backspace restoring original char
neovim_test!(scenarios, fh_R_backspace_restores, "hello", "Rxyz<BS><BS><BS>");
neovim_test!(scenarios, fh_R_backspace_partial, "hello", "Rxy<BS>");
neovim_test!(scenarios, fh_R_past_eol, "hello", "$Rxy<Esc>");
neovim_test!(scenarios, fh_R_with_newline, "hello", "R\n<Esc>");
neovim_test!(scenarios, fh_R_dot_repeat_basic, "hello\nworld", "Rxy<Esc>j.");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATOR + MOTION THAT CROSSES LINES
// ═══════════════════════════════════════════════════════════════════════════════

// d/pattern — delete to search result
neovim_test!(scenarios, fh_d_slash_pattern, "hello world test foo", "d/test<CR>");
neovim_test!(scenarios, fh_d_slash_next_line, "hello\nworld\ntest", "d/test<CR>");
neovim_test!(scenarios, fh_c_slash_pattern, "hello world test", "c/test<CR>X<Esc>");
neovim_test!(scenarios, fh_y_slash_pattern, "hello world test", "y/test<CR>$p");

// d with mark motion
neovim_test!(scenarios, fh_d_backtick_mark, "hello\nworld\ntest\nfoo", cursor(2, 0), "mawggd`a");
neovim_test!(scenarios, fh_d_quote_mark, "hello\nworld\ntest\nfoo", cursor(2, 0), "mawggd'a");

// ═══════════════════════════════════════════════════════════════════════════════
// MULTI-STEP UNDO/REDO INTERACTIONS
// ═══════════════════════════════════════════════════════════════════════════════

// Undo, then new edit invalidates redo stack
neovim_test!(scenarios, fh_undo_then_new_edit_clears_redo, "hello", "A world<Esc>u$a!<Esc><C-r>");
neovim_test!(scenarios, fh_multiple_undo_then_redo, "hello", "A one<Esc>A two<Esc>A three<Esc>uuu<C-r><C-r>");

// Undo should restore cursor position
neovim_test!(scenarios, fh_undo_restores_cursor_dd, "  hello\n  world\n  test", cursor(1, 0), "ddu");
neovim_test!(scenarios, fh_undo_restores_cursor_cw, "hello world", "cwXXX<Esc>u");
neovim_test!(scenarios, fh_undo_restores_cursor_o, "hello", "oworld<Esc>u");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES WITH WHITESPACE AND INDENTATION
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fh_dd_indented_cursor_pos, "    hello\n    world\n    test", "dd");
neovim_test!(scenarios, fh_cc_preserves_indent, "    hello", "ccworld<Esc>");
neovim_test!(scenarios, fh_S_preserves_indent, "    hello", "Sworld<Esc>");
neovim_test!(scenarios, fh_o_inherits_indent, "    hello", "oworld<Esc>");
neovim_test!(scenarios, fh_O_inherits_indent, "    hello", "Oworld<Esc>");
neovim_test!(scenarios, fh_J_with_leading_whitespace, "hello\n    world", "J");
neovim_test!(scenarios, fh_J_multiple_spaces, "hello   \n   world", "J");
neovim_test!(scenarios, fh_gJ_preserves_whitespace, "hello\n    world", "gJ");

// Tabs in various operations
neovim_test!(scenarios, fh_dw_on_tab, "\thello", "dw");
neovim_test!(scenarios, fh_cw_on_tab, "\thello", "cwX<Esc>");
neovim_test!(scenarios, fh_x_on_tab, "\thello", "x");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL BLOCK CURSOR POSITIONING
// ═══════════════════════════════════════════════════════════════════════════════

// Block select on lines shorter than selection
neovim_test!(scenarios, fh_vblock_short_line_delete, "hello\nhi\nworld", "<C-v>jj3ld");
neovim_test!(scenarios, fh_vblock_short_line_change, "hello\nhi\nworld", "<C-v>jj3lcX<Esc>");
neovim_test!(scenarios, fh_vblock_dollar_delete, "hello\nhi\nworld", "<C-v>jj$d");
neovim_test!(scenarios, fh_vblock_I_short_lines, "hello\nhi\nworld", "<C-v>jjIX<Esc>");
neovim_test!(scenarios, fh_vblock_A_short_lines, "hello\nhi\nworld", "<C-v>jj$AX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// INTERACTION BETWEEN OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

// Delete then paste back (should be identity with correct cursor)
neovim_test!(scenarios, fh_diw_then_P, "hello world test", cursor(0, 6), "diwP");
neovim_test!(scenarios, fh_dd_then_P, "hello\nworld\ntest", cursor(1, 0), "ddP");
neovim_test!(scenarios, fh_daw_then_p, "hello world test", "dawwp");

// Yank, change, then paste original
neovim_test!(scenarios, fh_yiw_cw_p, "hello world test", "yiwwcw<C-r>0<Esc>");
neovim_test!(scenarios, fh_yi_paren_then_ci_paren, "(hello) world", "yi(wci(<C-r>0<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CTRL-O IN INSERT MODE (single normal command)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fh_ctrl_o_in_insert_dw, "hello world test", "wi<C-o>dw");
neovim_test!(scenarios, fh_ctrl_o_in_insert_j, "hello\nworld", "A<C-o>j");
neovim_test!(scenarios, fh_ctrl_o_in_insert_dd, "hello\nworld\ntest", "ji<C-o>dd");
neovim_test!(scenarios, fh_ctrl_o_in_insert_o, "hello\nworld", "A<C-o>o");
neovim_test!(scenarios, fh_ctrl_o_in_insert_escape, "hello", "i<C-o><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT MODE SPECIAL KEYS
// ═══════════════════════════════════════════════════════════════════════════════

// Ctrl-W (delete word back) in insert mode
neovim_test!(scenarios, fh_insert_ctrl_w_basic, "hello world", "A<C-w>");
neovim_test!(scenarios, fh_insert_ctrl_w_twice, "hello world test", "A<C-w><C-w>");
neovim_test!(scenarios, fh_insert_ctrl_w_at_bol, "hello", "i<C-w>");
neovim_test!(scenarios, fh_insert_ctrl_w_after_spaces, "hello   ", "A<C-w>");

// Ctrl-U (delete to bol) in insert mode
neovim_test!(scenarios, fh_insert_ctrl_u_basic, "hello world", "A<C-u>");
neovim_test!(scenarios, fh_insert_ctrl_u_at_bol, "hello", "i<C-u>");
neovim_test!(scenarios, fh_insert_ctrl_u_mid, "hello world", cursor(0, 5), "a<C-u>");

// Ctrl-R (paste register) in insert mode
neovim_test!(scenarios, fh_insert_ctrl_r_unnamed, "hello world", "ywA <C-r>\"<Esc>");
neovim_test!(scenarios, fh_insert_ctrl_r_named, "hello", "\"aywA <C-r>a<Esc>");
neovim_test!(scenarios, fh_insert_ctrl_r_0, "hello world", "ywddA<C-r>0<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// MACRO EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

// Macro that changes text length, replayed multiple times
neovim_test!(scenarios, fh_macro_length_change, "1\n2\n3\n4\n5", "qa<C-a>jq4@a");
neovim_test!(scenarios, fh_macro_with_search, "aaa\nbbb\naaa\nbbb", "qa/bbb<CR>dd<Esc>q@a");
neovim_test!(scenarios, fh_macro_insert_mode, "hello", "qaiX<Esc>q3@a");
neovim_test!(scenarios, fh_macro_dot_inside, "abc\nabc\nabc", "qacwX<Esc>jq2@a");

// ═══════════════════════════════════════════════════════════════════════════════
// COMBINED COUNT EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

// Count multiplication: 2d3w should delete 6 words
neovim_test!(scenarios, fh_count_multiply_d_w, "one two three four five six seven eight", "2d3w");
neovim_test!(scenarios, fh_count_multiply_d_j, "a\nb\nc\nd\ne\nf\ng", "2d3j");
neovim_test!(scenarios, fh_count_2c3w, "one two three four five six seven eight", "2c3wX<Esc>");
neovim_test!(scenarios, fh_count_99x, "hello", "99x");
neovim_test!(scenarios, fh_count_99dd, "a\nb\nc", "99dd");
neovim_test!(scenarios, fh_count_99j, "a\nb\nc", "99j");
neovim_test!(scenarios, fh_count_zero_is_bol, "hello world", cursor(0, 5), "0");

// ═══════════════════════════════════════════════════════════════════════════════
// TILDE (~) EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fh_tilde_at_eol, "hello", cursor(0, 4), "~");
neovim_test!(scenarios, fh_tilde_on_digit, "a1b2c", cursor(0, 1), "~");
neovim_test!(scenarios, fh_tilde_on_space, "a b c", cursor(0, 1), "~");
neovim_test!(scenarios, fh_tilde_repeated, "hello", "~~~~~");
neovim_test!(scenarios, fh_tilde_with_count, "hello", "5~");
neovim_test!(scenarios, fh_g_tilde_w, "hello world", "g~w");
neovim_test!(scenarios, fh_g_tilde_tilde, "Hello World", "g~~");
neovim_test!(scenarios, fh_g_tilde_G, "Hello\nWorld\nTest", "g~G");
neovim_test!(scenarios, fh_gU_w, "hello world", "gUw");
neovim_test!(scenarios, fh_gu_w, "HELLO WORLD", "guw");
neovim_test!(scenarios, fh_gUU, "hello world", "gUU");
neovim_test!(scenarios, fh_guu, "HELLO WORLD", "guu");

// ═══════════════════════════════════════════════════════════════════════════════
// ZZ, ZQ, :wq, :q! (Should produce quit effects)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fh_ZZ_after_edit, "hello", "Aworld<Esc>ZZ");
neovim_test!(scenarios, fh_ZQ_after_edit, "hello", "Aworld<Esc>ZQ");

// ═══════════════════════════════════════════════════════════════════════════════
// LINEWISE vs CHARWISE DISTINCTION IN OPERATORS
// ═══════════════════════════════════════════════════════════════════════════════

// yj is linewise, y2l is charwise — paste behavior differs
neovim_test!(scenarios, fh_yj_paste_is_linewise, "hello\nworld\ntest", "yjjp");
neovim_test!(scenarios, fh_y2l_paste_is_charwise, "hello\nworld", "y2ljp");
neovim_test!(scenarios, fh_dj_is_linewise, "hello\nworld\ntest", "dj");
neovim_test!(scenarios, fh_d2l_is_charwise, "hello\nworld", "d2l");

// y$ is charwise, yy is linewise
neovim_test!(scenarios, fh_y_dollar_vs_yy, "hello\nworld", "y$jp");
neovim_test!(scenarios, fh_yy_paste_below, "hello\nworld", "yyjp");

// ═══════════════════════════════════════════════════════════════════════════════
// gn/gN — SELECT NEXT/PREV SEARCH MATCH
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fh_gn_basic, "hello world hello", "/hello<CR>gn");
neovim_test!(scenarios, fh_gn_delete, "hello world hello test", "/hello<CR>dgn");
neovim_test!(scenarios, fh_gn_change, "hello world hello test", "/hello<CR>cgnX<Esc>");
neovim_test!(scenarios, fh_gn_dot_repeat, "hello world hello test hello", "/hello<CR>cgnX<Esc>.");
neovim_test!(scenarios, fh_gN_basic, "hello world hello", "/hello<CR>$gN");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES: SINGLE CHARACTER DOCUMENT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fh_single_char_dw, "a", "dw");
neovim_test!(scenarios, fh_single_char_diw, "a", "diw");
neovim_test!(scenarios, fh_single_char_cw, "a", "cwX<Esc>");
neovim_test!(scenarios, fh_single_char_r, "a", "rx");
neovim_test!(scenarios, fh_single_char_yl_p, "a", "ylp");
neovim_test!(scenarios, fh_single_char_tilde, "a", "~");
neovim_test!(scenarios, fh_single_char_yy_p, "a", "yyp");
neovim_test!(scenarios, fh_single_char_indent, "a", ">>");
neovim_test!(scenarios, fh_single_char_J, "a\nb", "J");

// ═══════════════════════════════════════════════════════════════════════════════
// LAST LINE / EOF EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fh_o_on_last_line, "hello\nworld", cursor(1, 0), "otest<Esc>");
neovim_test!(scenarios, fh_p_on_last_line_linewise, "hello\nworld", cursor(1, 0), "yyp");
neovim_test!(scenarios, fh_J_on_last_line, "hello\nworld", cursor(1, 0), "J");
neovim_test!(scenarios, fh_dj_on_last_line, "hello\nworld", cursor(1, 0), "dj");
neovim_test!(scenarios, fh_yj_on_last_line, "hello\nworld", cursor(1, 0), "yj$p");
neovim_test!(scenarios, fh_dw_at_eof, "hello world", cursor(0, 6), "dw");
neovim_test!(scenarios, fh_x_at_eof, "hello", cursor(0, 4), "x");

// ═══════════════════════════════════════════════════════════════════════════════
// NO-TRAILING-NEWLINE DOCUMENT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fh_dd_no_trailing_nl, "hello\nworld", cursor(1, 0), "dd");
neovim_test!(scenarios, fh_dG_no_trailing_nl, "hello\nworld", "dG");
neovim_test!(scenarios, fh_p_at_eof_no_nl, "hello", "yyp");
neovim_test!(scenarios, fh_o_eof_no_nl, "hello", "oworld<Esc>");
neovim_test!(scenarios, fh_A_eof_no_nl, "hello", "A world<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// SUBSTITUTE / SEARCH EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fh_star_at_end_of_word, "hello world hello", cursor(0, 3), "*");
neovim_test!(scenarios, fh_hash_at_end_of_word, "hello world hello", cursor(0, 3), "#");
neovim_test!(scenarios, fh_star_on_punctuation, "a.b a.b", "*");
neovim_test!(scenarios, fh_n_wraps_around, "hello\nworld\nhello", "/hello<CR>n");
neovim_test!(scenarios, fh_N_wraps_around, "hello\nworld\nhello", "/hello<CR>N");
neovim_test!(scenarios, fh_search_then_dot, "hello world hello world", "/world<CR>cwX<Esc>n.");
