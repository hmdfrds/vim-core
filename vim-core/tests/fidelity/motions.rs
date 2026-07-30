// Motion fidelity tests for vim-core.
//
// These tests compare vim-core output against Neovim oracle.
// 107 tests covering all motion categories per phase-5-motions.md.
//
// Test Categories:
// - Character (h,l,0,$,^,g_): 20 tests
// - Line (j,k,+,-,gj,gk): 20 tests
// - Word (w,b,e,ge,W,B,E,gE): 30 tests
// - Document (gg,G,%,H,M,L,_): 15 tests
// - Find (f,F,t,T,;,,): 25 tests
// - Scroll (Ctrl-D,Ctrl-U,Ctrl-F,Ctrl-B,zz): 15 tests

// Note: We're included via include!() so we use super::common directly
// Do not add `use super::common;` here

// ═══════════════════════════════════════════════════════════════════════════════
// CHARACTER MOTIONS (h, l, 0, $, ^, g_)
// ═══════════════════════════════════════════════════════════════════════════════

// h - move left
neovim_test!(motions, h_basic, "hello", cursor(0, 2), "h");
neovim_test!(motions, h_at_start, "hello", cursor(0, 0), "h");
neovim_test!(motions, h_with_count, "hello world", cursor(0, 6), "3h");
neovim_test!(motions, h_count_past_start, "hello", cursor(0, 2), "10h");
// NEW: h edge cases
neovim_test!(motions, h_empty_buffer, "", "h");
neovim_test!(motions, h_single_char, "a", "h");
neovim_test!(motions, h_at_eol, "hello", cursor(0, 4), "h");
neovim_test!(motions, h_count_1, "hello", cursor(0, 2), "1h");
neovim_test!(motions, h_large_count, "hello world", cursor(0, 10), "100h");
neovim_test!(motions, h_unicode_cjk, "日本語", cursor(0, 2), "h");
neovim_test!(motions, h_unicode_emoji, "🎉🎉🎉", cursor(0, 2), "h");

// l - move right
neovim_test!(motions, l_basic, "hello", "l");
neovim_test!(motions, l_at_end, "hello", cursor(0, 4), "l");
neovim_test!(motions, l_with_count, "hello world", "3l");
neovim_test!(motions, l_count_past_end, "hello", cursor(0, 3), "10l");
// NEW: l edge cases
neovim_test!(motions, l_empty_buffer, "", "l");
neovim_test!(motions, l_single_char, "a", "l");
neovim_test!(motions, l_at_bol, "hello", "l");
neovim_test!(motions, l_count_1, "hello", "1l");
neovim_test!(motions, l_large_count, "hello world", "100l");
neovim_test!(motions, l_unicode_cjk, "日本語", "l");
neovim_test!(motions, l_unicode_emoji, "🎉🎉🎉", "l");

// 0 - line start
neovim_test!(motions, zero_basic, "hello", cursor(0, 3), "0");
neovim_test!(motions, zero_indented, "  hello", cursor(0, 4), "0");
neovim_test!(motions, zero_multiline, "line1\n  line2", cursor(1, 4), "0");
// NEW: 0 edge cases
neovim_test!(motions, zero_empty_buffer, "", "0");
neovim_test!(motions, zero_single_char, "a", "0");
neovim_test!(motions, zero_at_eol, "hello", cursor(0, 4), "0");
neovim_test!(motions, zero_tabs_only, "\t\t\t", cursor(0, 2), "0");
neovim_test!(motions, zero_unicode, "日本語 hello", cursor(0, 5), "0");

// $ - line end
neovim_test!(motions, dollar_basic, "hello", "$");
neovim_test!(motions, dollar_at_end, "hello", cursor(0, 4), "$");
neovim_test!(motions, dollar_empty_line, "\n", "$");
neovim_test!(motions, dollar_multiline, "short\nlonger line", cursor(1, 0), "$");
// NEW: $ edge cases
neovim_test!(motions, dollar_single_char, "a", "$");
neovim_test!(motions, dollar_at_bol, "hello", "$");
neovim_test!(motions, dollar_with_count, "line1\nline2\nline3", "2$");
neovim_test!(motions, dollar_unicode_cjk, "日本語", "$");
neovim_test!(motions, dollar_unicode_emoji, "🎉🎉🎉", "$");
neovim_test!(motions, dollar_trailing_space, "hello   ", "$");
neovim_test!(motions, dollar_only_whitespace, "     ", "$");

// ^ - first non-blank
neovim_test!(motions, caret_basic, "  hello", cursor(0, 5), "^");
neovim_test!(motions, caret_no_indent, "hello", cursor(0, 3), "^");
neovim_test!(motions, caret_tab_indent, "\thello", cursor(0, 3), "^");
neovim_test!(motions, caret_all_blank, "   \n", cursor(0, 0), "^");
// NEW: ^ edge cases
neovim_test!(motions, caret_empty_buffer, "", "^");
neovim_test!(motions, caret_single_char, "a", "^");
neovim_test!(motions, caret_single_space_char, " a", "^");
neovim_test!(motions, caret_mixed_indent, "  \t  hello", cursor(0, 8), "^");
neovim_test!(motions, caret_from_eol, "  hello", cursor(0, 6), "^");
neovim_test!(motions, caret_unicode, "  日本語", cursor(0, 4), "^");

// g_ - last non-blank
neovim_test!(motions, g_underscore_basic, "hello  ", "g_");
neovim_test!(motions, g_underscore_no_trailing, "hello", "g_");
// NEW: g_ edge cases
neovim_test!(motions, g_underscore_empty_buffer, "", "g_");
neovim_test!(motions, g_underscore_single_char, "a", "g_");
neovim_test!(motions, g_underscore_all_whitespace, "     ", "g_");
neovim_test!(motions, g_underscore_tabs_trailing, "hello\t\t", "g_");
neovim_test!(motions, g_underscore_unicode, "日本語  ", "g_");
neovim_test!(motions, g_underscore_from_middle, "hello   ", cursor(0, 2), "g_");

// ═══════════════════════════════════════════════════════════════════════════════
// LINE MOTIONS (j, k, +, -, gj, gk)
// ═══════════════════════════════════════════════════════════════════════════════

// j - move down
neovim_test!(motions, j_basic, "line1\nline2", "j");
neovim_test!(motions, j_at_last_line, "line1\nline2", cursor(1, 0), "j");
neovim_test!(motions, j_with_count, "l1\nl2\nl3\nl4", "2j");
neovim_test!(motions, j_preserves_column, "hello\nworld", cursor(0, 3), "j");
neovim_test!(motions, j_clamps_short_line, "hello\nhi\nworld", cursor(0, 4), "j");
// NEW: j edge cases
neovim_test!(motions, j_empty_buffer, "", "j");
neovim_test!(motions, j_single_line, "hello", "j");
neovim_test!(motions, j_count_past_eof, "l1\nl2\nl3", "100j");
neovim_test!(motions, j_column_restore, "hello\nhi\nhello", cursor(0, 4), "jj");
neovim_test!(motions, j_count_1, "l1\nl2", "1j");
neovim_test!(motions, j_unicode_line, "日本語\nhello", "j");
neovim_test!(motions, j_empty_lines, "\n\n\n", "j");
neovim_test!(motions, j_with_tabs, "hello\t\nworld\t", "j");

// k - move up
neovim_test!(motions, k_basic, "line1\nline2", cursor(1, 0), "k");
neovim_test!(motions, k_at_first_line, "line1\nline2", "k");
neovim_test!(motions, k_with_count, "l1\nl2\nl3\nl4", cursor(3, 0), "2k");
neovim_test!(motions, k_preserves_column, "hello\nworld", cursor(1, 3), "k");
// NEW: k edge cases
neovim_test!(motions, k_empty_buffer, "", "k");
neovim_test!(motions, k_single_line, "hello", "k");
neovim_test!(motions, k_count_past_bof, "l1\nl2\nl3", cursor(2, 0), "100k");
neovim_test!(motions, k_column_restore, "hello\nhi\nhello", cursor(2, 4), "kk");
neovim_test!(motions, k_count_1, "l1\nl2", cursor(1, 0), "1k");
neovim_test!(motions, k_unicode_line, "hello\n日本語", cursor(1, 0), "k");
neovim_test!(motions, k_empty_lines, "\n\n\n", cursor(2, 0), "k");

// + - down to first non-blank
neovim_test!(motions, plus_basic, "line1\n  line2", "+");
neovim_test!(motions, plus_no_indent, "line1\nline2", "+");
neovim_test!(motions, plus_with_count, "l1\n  l2\n    l3", "2+");
// NEW: + edge cases
neovim_test!(motions, plus_empty_buffer, "", "+");
neovim_test!(motions, plus_single_line, "hello", "+");
neovim_test!(motions, plus_at_last_line, "l1\nl2", cursor(1, 0), "+");
neovim_test!(motions, plus_tabs, "l1\n\t\tl2", "+");
neovim_test!(motions, plus_empty_next_line, "hello\n\nworld", "+");

// - - up to first non-blank
neovim_test!(motions, minus_basic, "  line1\nline2", cursor(1, 0), "-");
neovim_test!(motions, minus_with_count, "l1\n  l2\n    l3", cursor(2, 0), "2-");
// NEW: - edge cases
neovim_test!(motions, minus_empty_buffer, "", "-");
neovim_test!(motions, minus_single_line, "hello", "-");
neovim_test!(motions, minus_at_first_line, "l1\nl2", "-");
neovim_test!(motions, minus_tabs, "\t\tl1\nl2", cursor(1, 0), "-");
neovim_test!(motions, minus_empty_prev_line, "hello\n\nworld", cursor(2, 0), "-");

// gj - display line down (same as j for non-wrapped)
neovim_test!(motions, gj_basic, "line1\nline2", "gj");
// NEW: gj edge cases
neovim_test!(motions, gj_empty_buffer, "", "gj");
neovim_test!(motions, gj_single_line, "hello", "gj");
neovim_test!(motions, gj_at_last_line, "l1\nl2", cursor(1, 0), "gj");
neovim_test!(motions, gj_with_count, "l1\nl2\nl3\nl4", "2gj");

// gk - display line up (same as k for non-wrapped)
neovim_test!(motions, gk_basic, "line1\nline2", cursor(1, 0), "gk");
// NEW: gk edge cases
neovim_test!(motions, gk_empty_buffer, "", "gk");
neovim_test!(motions, gk_single_line, "hello", "gk");
neovim_test!(motions, gk_at_first_line, "l1\nl2", "gk");
neovim_test!(motions, gk_with_count, "l1\nl2\nl3\nl4", cursor(3, 0), "2gk");

// ═══════════════════════════════════════════════════════════════════════════════
// WORD MOTIONS (w, b, e, ge, W, B, E, gE)
// ═══════════════════════════════════════════════════════════════════════════════

// w - next word start
neovim_test!(motions, w_basic, "hello world", "w");
neovim_test!(motions, w_punctuation, "foo.bar baz", "w");
neovim_test!(motions, w_with_count, "one two three", "2w");
neovim_test!(motions, w_at_end, "hello", cursor(0, 4), "w");
neovim_test!(motions, w_across_lines, "word\nnext", cursor(0, 2), "w");
neovim_test!(motions, w_multiple_spaces, "hello   world", "w");
// NEW: w edge cases
neovim_test!(motions, w_empty_buffer, "", "w");
neovim_test!(motions, w_single_char, "a", "w");
neovim_test!(motions, w_single_word, "hello", "w");
neovim_test!(motions, w_large_count, "one two three", "100w");
neovim_test!(motions, w_punct_only, "...", "w");
neovim_test!(motions, w_mixed_punct, "foo...bar", "w");
neovim_test!(motions, w_unicode_cjk, "日本語 hello", "w");
neovim_test!(motions, w_unicode_emoji, "🎉🎉 hello", "w");
neovim_test!(motions, w_tabs, "hello\tworld", "w");
neovim_test!(motions, w_mixed_whitespace, "hello \t\n world", "w");
neovim_test!(motions, w_at_punct, "foo.bar", cursor(0, 3), "w");
neovim_test!(motions, w_line_start_indent, "\n  hello", "w");

// e - word end
neovim_test!(motions, e_basic, "hello world", "e");
neovim_test!(motions, e_from_middle, "hello world", cursor(0, 2), "e");
neovim_test!(motions, e_with_count, "one two three", "2e");
neovim_test!(motions, e_punctuation, "foo.bar", "e");
// NEW: e edge cases
neovim_test!(motions, e_empty_buffer, "", "e");
neovim_test!(motions, e_single_char, "a", "e");
neovim_test!(motions, e_single_word, "hello", "e");
neovim_test!(motions, e_at_end, "hello", cursor(0, 4), "e");
neovim_test!(motions, e_large_count, "one two three", "100e");
neovim_test!(motions, e_across_lines, "word\nnext", cursor(0, 3), "e");
neovim_test!(motions, e_unicode_cjk, "日本語", "e");
neovim_test!(motions, e_unicode_emoji, "🎉🎉🎉", "e");
neovim_test!(motions, e_punct_only, "...", "e");
neovim_test!(motions, e_at_word_start, "hello world", cursor(0, 6), "e");

// b - word backward
neovim_test!(motions, b_basic, "hello world", cursor(0, 6), "b");
neovim_test!(motions, b_at_start, "hello world", "b");
neovim_test!(motions, b_with_count, "one two three", cursor(0, 8), "2b");
neovim_test!(motions, b_punctuation, "foo.bar", cursor(0, 4), "b");
// NEW: b edge cases
neovim_test!(motions, b_empty_buffer, "", "b");
neovim_test!(motions, b_single_char, "a", "b");
neovim_test!(motions, b_single_word, "hello", cursor(0, 4), "b");
neovim_test!(motions, b_large_count, "one two three", cursor(0, 8), "100b");
neovim_test!(motions, b_across_lines, "word\nnext", cursor(1, 2), "b");
neovim_test!(motions, b_unicode_cjk, "hello 日本語", cursor(0, 8), "b");
neovim_test!(motions, b_unicode_emoji, "hello 🎉🎉", cursor(0, 8), "b");
neovim_test!(motions, b_punct_only, "...", cursor(0, 2), "b");
neovim_test!(motions, b_at_word_end, "hello world", cursor(0, 10), "b");
neovim_test!(motions, b_multiple_punct, "a..b..c", cursor(0, 6), "b");

// ge - word end backward
neovim_test!(motions, ge_basic, "hello world foo", cursor(0, 12), "ge");
neovim_test!(motions, ge_at_start, "hello", "ge");
// NEW: ge edge cases
neovim_test!(motions, ge_empty_buffer, "", "ge");
neovim_test!(motions, ge_single_char, "a", "ge");
neovim_test!(motions, ge_single_word, "hello", cursor(0, 4), "ge");
neovim_test!(motions, ge_with_count, "one two three", cursor(0, 12), "2ge");
neovim_test!(motions, ge_across_lines, "word\nnext", cursor(1, 0), "ge");
neovim_test!(motions, ge_unicode_cjk, "hello 日本語", cursor(0, 8), "ge");
neovim_test!(motions, ge_punct, "foo.bar", cursor(0, 4), "ge");

// W - WORD forward
neovim_test!(motions, W_basic, "foo.bar baz", "W");
neovim_test!(motions, W_with_count, "a.b c.d e.f", "2W");
neovim_test!(motions, W_vs_w, "foo.bar", "W");
// NEW: W edge cases
neovim_test!(motions, W_empty_buffer, "", "W");
neovim_test!(motions, W_single_char, "a", "W");
neovim_test!(motions, W_at_end, "hello", cursor(0, 4), "W");
neovim_test!(motions, W_large_count, "a.b c.d e.f", "100W");
neovim_test!(motions, W_across_lines, "foo.bar\nbaz.qux", "W");
neovim_test!(motions, W_unicode_cjk, "日本語.test hello", "W");
neovim_test!(motions, W_tabs, "foo.bar\tbaz", "W");

// E - WORD end
neovim_test!(motions, E_basic, "foo.bar baz", "E");
neovim_test!(motions, E_vs_e, "foo.bar baz", cursor(0, 2), "E");
// NEW: E edge cases
neovim_test!(motions, E_empty_buffer, "", "E");
neovim_test!(motions, E_single_char, "a", "E");
neovim_test!(motions, E_at_end, "foo.bar", cursor(0, 6), "E");
neovim_test!(motions, E_with_count, "a.b c.d e.f", "2E");
neovim_test!(motions, E_large_count, "a.b c.d e.f", "100E");
neovim_test!(motions, E_across_lines, "foo.bar\nbaz.qux", "E");
neovim_test!(motions, E_unicode_cjk, "日本語.test", "E");

// B - WORD backward
neovim_test!(motions, B_basic, "foo.bar baz.qux", cursor(0, 8), "B");
neovim_test!(motions, B_vs_b, "foo.bar", cursor(0, 6), "B");
// NEW: B edge cases
neovim_test!(motions, B_empty_buffer, "", "B");
neovim_test!(motions, B_single_char, "a", "B");
neovim_test!(motions, B_at_start, "foo.bar", "B");
neovim_test!(motions, B_with_count, "a.b c.d e.f", cursor(0, 8), "2B");
neovim_test!(motions, B_large_count, "a.b c.d e.f", cursor(0, 8), "100B");
neovim_test!(motions, B_across_lines, "foo.bar\nbaz.qux", cursor(1, 4), "B");
neovim_test!(motions, B_unicode_cjk, "hello 日本語.test", cursor(0, 12), "B");

// gE - WORD end backward
neovim_test!(motions, gE_basic, "foo.bar baz.qux", cursor(0, 12), "gE");
// NEW: gE edge cases
neovim_test!(motions, gE_empty_buffer, "", "gE");
neovim_test!(motions, gE_single_char, "a", "gE");
neovim_test!(motions, gE_at_start, "foo.bar", "gE");
neovim_test!(motions, gE_with_count, "a.b c.d e.f", cursor(0, 10), "2gE");
neovim_test!(motions, gE_across_lines, "foo.bar\nbaz.qux", cursor(1, 0), "gE");
neovim_test!(motions, gE_unicode_cjk, "hello 日本語.test", cursor(0, 12), "gE");

// ═══════════════════════════════════════════════════════════════════════════════
// DOCUMENT MOTIONS (gg, G, %, H, M, L, _)
// ═══════════════════════════════════════════════════════════════════════════════

// gg - go to first line
neovim_test!(motions, gg_basic, "line1\nline2\nline3", cursor(2, 0), "gg");
neovim_test!(motions, gg_with_count, "line1\nline2\nline3", "2gg");
neovim_test!(motions, gg_indented, "line1\n  line2\nline3", "2gg");
// NEW: gg edge cases
neovim_test!(motions, gg_empty_buffer, "", "gg");
neovim_test!(motions, gg_single_line, "hello", "gg");
neovim_test!(motions, gg_at_first_line, "line1\nline2", "gg");
neovim_test!(motions, gg_count_past_eof, "l1\nl2\nl3", "100gg");
neovim_test!(motions, gg_count_0, "l1\nl2\nl3", "0gg");
neovim_test!(motions, gg_count_1, "l1\nl2\nl3", cursor(2, 0), "1gg");
neovim_test!(motions, gg_with_indent, "  line1\n  line2", cursor(1, 0), "gg");

// G - go to last line
neovim_test!(motions, G_basic, "line1\nline2\nline3", "G");
neovim_test!(motions, G_with_count, "line1\nline2\nline3", "2G");
neovim_test!(motions, G_indented, "line1\n  line2\nline3", "G");
// NEW: G edge cases
neovim_test!(motions, G_empty_buffer, "", "G");
neovim_test!(motions, G_single_line, "hello", "G");
neovim_test!(motions, G_at_last_line, "line1\nline2", cursor(1, 0), "G");
neovim_test!(motions, G_count_past_eof, "l1\nl2\nl3", "100G");
neovim_test!(motions, G_count_0, "l1\nl2\nl3", "0G");
neovim_test!(motions, G_count_1, "l1\nl2\nl3", "1G");

// % - percent and matching bracket
neovim_test!(motions, percent_50, "l1\nl2\nl3\nl4", "50%");
neovim_test!(motions, percent_100, "l1\nl2\nl3", "100%");
neovim_test!(motions, percent_bracket, "(hello)", "%");
neovim_test!(motions, percent_nested, "((inner))", "%");
// NEW: % edge cases
neovim_test!(motions, percent_empty_buffer, "", "%");
neovim_test!(motions, percent_no_bracket, "hello", "%");
neovim_test!(motions, percent_brace, "{hello}", "%");
neovim_test!(motions, percent_bracket_square, "[hello]", "%");
neovim_test!(motions, percent_angle, "<hello>", "%");
neovim_test!(motions, percent_unmatched, "(hello", "%");
neovim_test!(motions, percent_deep_nesting, "((((x))))", "%");
neovim_test!(motions, percent_on_closing, "(hello)", cursor(0, 6), "%");
neovim_test!(motions, percent_mixed, "({[<>]})", "%");
neovim_test!(motions, percent_1, "l1\nl2\nl3\nl4\nl5", "1%");
neovim_test!(motions, percent_25, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8", "25%");
neovim_test!(motions, percent_75, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8", "75%");

// _ - first non-blank with count
neovim_test!(motions, underscore_basic, "  hello", "_");
neovim_test!(motions, underscore_count, "line1\n  line2\nline3", "2_");
// NEW: _ edge cases
neovim_test!(motions, underscore_empty_buffer, "", "_");
neovim_test!(motions, underscore_single_line, "hello", "_");
neovim_test!(motions, underscore_count_past_eof, "l1\nl2", "100_");
neovim_test!(motions, underscore_no_indent, "hello\nworld", "2_");
neovim_test!(motions, underscore_tabs, "l1\n\t\tl2", "2_");

// H - high (top of screen)
neovim_test!(motions, H_basic, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(5, 0), "H");
neovim_test!(motions, H_with_count, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(5, 0), "2H");
neovim_test!(motions, H_empty_buffer, "", "H");
neovim_test!(motions, H_single_line, "hello", "H");

// M - middle of screen
neovim_test!(motions, M_basic, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(0, 0), "M");
neovim_test!(motions, M_empty_buffer, "", "M");
neovim_test!(motions, M_single_line, "hello", "M");

// L - low (bottom of screen)
neovim_test!(motions, L_basic, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(0, 0), "L");
neovim_test!(motions, L_with_count, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(0, 0), "2L");
neovim_test!(motions, L_empty_buffer, "", "L");
neovim_test!(motions, L_single_line, "hello", "L");

// ═══════════════════════════════════════════════════════════════════════════════
// FIND MOTIONS (f, F, t, T, ;, ,)
// ═══════════════════════════════════════════════════════════════════════════════

// f - find forward
neovim_test!(motions, f_basic, "hello world", "fo");
neovim_test!(motions, f_with_count, "hello world", "2o");
neovim_test!(motions, f_not_found, "hello", "fz");
neovim_test!(motions, f_same_char, "abcabc", "fa");
neovim_test!(motions, f_at_char, "hello", cursor(0, 2), "fl");
// NEW: f edge cases
neovim_test!(motions, f_empty_buffer, "", "fa");
neovim_test!(motions, f_single_char, "a", "fa");
neovim_test!(motions, f_at_eol, "hello", cursor(0, 4), "fo");
neovim_test!(motions, f_count_2, "aaa", "2fa");
neovim_test!(motions, f_count_past_end, "aa", "100fa");
neovim_test!(motions, f_unicode, "hello 日 world", "f日");
neovim_test!(motions, f_space, "hello world", "f ");
neovim_test!(motions, f_newline_boundary, "hello\nworld", "fw");
neovim_test!(motions, f_tab, "hello\tworld", "f\t");
neovim_test!(motions, f_same_pos, "aa", "fa");

// F - find backward
neovim_test!(motions, F_basic, "hello world", cursor(0, 10), "Fo");
neovim_test!(motions, F_at_start, "hello", "Fe");
neovim_test!(motions, F_with_count, "hello world", cursor(0, 10), "2Fo");
// NEW: F edge cases
neovim_test!(motions, F_empty_buffer, "", "Fa");
neovim_test!(motions, F_single_char, "a", "Fa");
neovim_test!(motions, F_at_bol, "hello", "Fh");
neovim_test!(motions, F_count_2, "aaa", cursor(0, 2), "2Fa");
neovim_test!(motions, F_count_past_start, "aa", cursor(0, 1), "100Fa");
neovim_test!(motions, F_unicode, "hello 日 world", cursor(0, 10), "F日");
neovim_test!(motions, F_space, "hello world", cursor(0, 10), "F ");
neovim_test!(motions, F_same_pos, "aa", cursor(0, 1), "Fa");

// t - till forward
neovim_test!(motions, t_basic, "hello world", "to");
neovim_test!(motions, t_with_count, "hello world", "2to");
neovim_test!(motions, t_not_found, "hello", "tz");
// NEW: t edge cases
neovim_test!(motions, t_empty_buffer, "", "ta");
neovim_test!(motions, t_single_char, "a", "ta");
neovim_test!(motions, t_adjacent, "ab", "tb");
neovim_test!(motions, t_count_2, "aaa", "2ta");
neovim_test!(motions, t_unicode, "hello 日 world", "t日");
neovim_test!(motions, t_at_target, "ab", cursor(0, 0), "ta");
neovim_test!(motions, t_same_char, "aa", "ta");

// T - till backward
neovim_test!(motions, T_basic, "hello world", cursor(0, 10), "To");
neovim_test!(motions, T_at_start, "hello", "Te");
// NEW: T edge cases
neovim_test!(motions, T_empty_buffer, "", "Ta");
neovim_test!(motions, T_single_char, "a", "Ta");
neovim_test!(motions, T_adjacent, "ab", cursor(0, 1), "Ta");
neovim_test!(motions, T_count_2, "aaa", cursor(0, 2), "2Ta");
neovim_test!(motions, T_unicode, "hello 日 world", cursor(0, 10), "T日");
neovim_test!(motions, T_at_target, "ab", cursor(0, 1), "Tb");

// ; - repeat find
neovim_test!(motions, semicolon_repeat, "abcabc", "fa;");
neovim_test!(motions, semicolon_no_previous, "hello", ";");
// NEW: ; edge cases
neovim_test!(motions, semicolon_empty_buffer, "", ";");
neovim_test!(motions, semicolon_after_F, "abcabc", cursor(0, 5), "Fa;");
neovim_test!(motions, semicolon_after_t, "abcabc", "ta;");
neovim_test!(motions, semicolon_after_T, "abcabc", cursor(0, 5), "Ta;");
neovim_test!(motions, semicolon_multiple, "aaaa", "fa;;");
neovim_test!(motions, semicolon_at_end, "aa", "fa;");

// , - repeat find reverse
neovim_test!(motions, comma_repeat_reverse, "abcabc", cursor(0, 3), "fa,");
// NEW: , edge cases
neovim_test!(motions, comma_empty_buffer, "", ",");
neovim_test!(motions, comma_no_previous, "hello", ",");
neovim_test!(motions, comma_after_f, "abcabc", cursor(0, 3), "fa,");
neovim_test!(motions, comma_after_F, "abcabc", cursor(0, 0), "Fa,");
neovim_test!(motions, comma_after_t, "abcabc", cursor(0, 3), "ta,");
neovim_test!(motions, comma_multiple, "aaaa", cursor(0, 3), "Fa,,");
neovim_test!(motions, comma_at_start, "aa", cursor(0, 1), "Fa,");

// ═══════════════════════════════════════════════════════════════════════════════
// SCROLL MOTIONS (Ctrl-D, Ctrl-U, Ctrl-F, Ctrl-B, zz)
// ═══════════════════════════════════════════════════════════════════════════════

// These produce scroll effects, test cursor position after scroll
neovim_test!(motions, ctrl_d_basic, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "<C-d>");
neovim_test!(motions, ctrl_u_basic, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(9, 0), "<C-u>");
neovim_test!(motions, ctrl_f_basic, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "<C-f>");
neovim_test!(motions, ctrl_b_basic, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(9, 0), "<C-b>");
// NEW: scroll edge cases
neovim_test!(motions, ctrl_d_empty_buffer, "", "<C-d>");
neovim_test!(motions, ctrl_d_single_line, "hello", "<C-d>");
neovim_test!(motions, ctrl_d_with_count, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "3<C-d>");
neovim_test!(motions, ctrl_u_empty_buffer, "", "<C-u>");
neovim_test!(motions, ctrl_u_single_line, "hello", "<C-u>");
neovim_test!(motions, ctrl_u_at_top, "l1\nl2\nl3\nl4\nl5", "<C-u>");
neovim_test!(motions, ctrl_f_empty_buffer, "", "<C-f>");
neovim_test!(motions, ctrl_f_single_line, "hello", "<C-f>");
neovim_test!(motions, ctrl_b_empty_buffer, "", "<C-b>");
neovim_test!(motions, ctrl_b_single_line, "hello", "<C-b>");
neovim_test!(motions, ctrl_e_basic, "l1\nl2\nl3\nl4\nl5", "<C-e>");
neovim_test!(motions, ctrl_y_basic, "l1\nl2\nl3\nl4\nl5", cursor(4, 0), "<C-y>");

// z commands
neovim_test!(motions, zz_center, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9", cursor(4, 0), "zz");
neovim_test!(motions, zt_top, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9", cursor(4, 0), "zt");
neovim_test!(motions, zb_bottom, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9", cursor(4, 0), "zb");
// NEW: z edge cases
neovim_test!(motions, zz_empty_buffer, "", "zz");
neovim_test!(motions, zz_single_line, "hello", "zz");
neovim_test!(motions, zt_empty_buffer, "", "zt");
neovim_test!(motions, zt_single_line, "hello", "zt");
neovim_test!(motions, zb_empty_buffer, "", "zb");
neovim_test!(motions, zb_single_line, "hello", "zb");

// ═══════════════════════════════════════════════════════════════════════════════
// PARAGRAPH AND SENTENCE MOTIONS ({, }, (, ))
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(motions, paragraph_forward, "line1\n\nline2", "}");
neovim_test!(motions, paragraph_backward, "line1\n\nline2", cursor(2, 0), "{");
neovim_test!(motions, sentence_forward, "Hello. World", ")");
// NEW: paragraph edge cases
neovim_test!(motions, para_forward_empty_buffer, "", "}");
neovim_test!(motions, para_forward_single_line, "hello", "}");
neovim_test!(motions, para_forward_no_blank, "line1\nline2\nline3", "}");
neovim_test!(motions, para_forward_multiple, "l1\n\nl2\n\nl3", "}");
neovim_test!(motions, para_forward_with_count, "l1\n\nl2\n\nl3", "2}");
neovim_test!(motions, para_forward_at_end, "l1\n\nl2", cursor(2, 0), "}");
neovim_test!(motions, para_forward_only_blanks, "\n\n\n", "}");
neovim_test!(motions, para_backward_empty_buffer, "", "{");
neovim_test!(motions, para_backward_single_line, "hello", "{");
neovim_test!(motions, para_backward_no_blank, "line1\nline2\nline3", cursor(2, 0), "{");
neovim_test!(motions, para_backward_multiple, "l1\n\nl2\n\nl3", cursor(4, 0), "2{");
neovim_test!(motions, para_backward_at_start, "l1\n\nl2", "{");
neovim_test!(motions, para_backward_only_blanks, "\n\n\n", cursor(2, 0), "{");
// NEW: sentence edge cases
neovim_test!(motions, sentence_forward_empty_buffer, "", ")");
neovim_test!(motions, sentence_forward_single_word, "hello", ")");
neovim_test!(motions, sentence_forward_period, "Hello. World. End", ")");
neovim_test!(motions, sentence_forward_exclaim, "Hello! World", ")");
neovim_test!(motions, sentence_forward_question, "Hello? World", ")");
neovim_test!(motions, sentence_forward_with_count, "One. Two. Three.", "2)");
neovim_test!(motions, sentence_backward, "Hello. World", cursor(0, 7), "(");
neovim_test!(motions, sentence_backward_empty_buffer, "", "(");
neovim_test!(motions, sentence_backward_at_start, "Hello. World", "(");
neovim_test!(motions, sentence_backward_with_count, "One. Two. Three.", cursor(0, 10), "2(");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES - COMPREHENSIVE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(motions, motions_single_char_edge, "a", "l");
neovim_test!(motions, unicode_emoji, "hello 🎉 world", "w");
neovim_test!(motions, unicode_grapheme, "héllo wörld", "w");
neovim_test!(motions, newline_only, "\n\n\n", "j");
neovim_test!(motions, tabs, "\t\thello", "^");
neovim_test!(motions, mixed_whitespace, "  \t  hello", "^");

// NEW: Additional Unicode edge cases
neovim_test!(motions, unicode_zwj_emoji, "👨‍👩‍👧 hello", "w");
neovim_test!(motions, unicode_combining, "e\u{0301}llo", "w"); // é as e + combining acute
neovim_test!(motions, unicode_cjk_full, "日本語テスト", "w");
neovim_test!(motions, unicode_korean, "한글테스트", "w");
neovim_test!(motions, unicode_arabic, "مرحبا", "w");
neovim_test!(motions, unicode_mixed_scripts, "hello日本語world", "w");

// NEW: Very long lines
neovim_test!(motions, long_line_start, "a b c d e f g h i j k l m n o p q r s t u v w x y z", "w");
neovim_test!(motions, long_line_end, "a b c d e f g h i j k l m n o p q r s t u v w x y z", cursor(0, 50), "b");

// NEW: Only special characters
neovim_test!(motions, only_punctuation, "...", "w");
neovim_test!(motions, only_spaces, "     ", "w");
neovim_test!(motions, only_tabs, "\t\t\t", "w");

// NEW: Motion sequences
neovim_test!(motions, sequence_hjkl, "hello\nworld\ntest", "llljjkh");
neovim_test!(motions, sequence_wb, "one two three", "wwwbb");
neovim_test!(motions, sequence_0_dollar, "  hello world  ", "0$0");
neovim_test!(motions, sequence_gg_G, "l1\nl2\nl3\nl4\nl5", "GggG");

// ─────────────────────────────────────────────────────────────────────────────
// Search Object Motions (gn, gN)
// ─────────────────────────────────────────────────────────────────────────────

// gn — visually select next search match (requires prior search)
neovim_test!(motions, gn_after_search, "foo bar foo baz foo", "/foo<CR>lgn");
neovim_test!(motions, gn_at_match, "foo bar foo baz", "/foo<CR>ww gn");
neovim_test!(motions, gn_wrap_around, "bar foo baz", "/foo<CR>$gn");
neovim_test!(motions, gn_no_match, "hello world", "/zzz<CR>gn");
neovim_test!(motions, dgn_delete_match, "foo bar foo baz", "/foo<CR>ww dgn");
neovim_test!(motions, cgn_change_match, "foo bar foo baz", "/foo<CR>ww cgnXX<Esc>");
neovim_test!(motions, cgn_dot_repeat, "foo bar foo baz foo", "/foo<CR>cgnXX<Esc>..");
neovim_test!(motions, gN_backward, "foo bar foo baz", "/foo<CR>$gN");
neovim_test!(motions, dgN_delete, "foo bar foo", "/foo<CR>$dgN");
neovim_test!(motions, cgN_change, "foo bar foo baz", "/foo<CR>$cgNYY<Esc>");

// ─────────────────────────────────────────────────────────────────────────────
// Changelist Motions (g; g,)
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(motions, g_semicolon_basic, "hello world test", "cwfoo<Esc>$g;");
neovim_test!(motions, g_semicolon_after_two_edits, "aaa bbb ccc", "cwXX<Esc>wcwYY<Esc>g;");
neovim_test!(motions, g_semicolon_twice, "aaa bbb ccc", "cwXX<Esc>wcwYY<Esc>$g;g;");
neovim_test!(motions, g_comma_basic, "hello world test", "cwfoo<Esc>$g;g,");
neovim_test!(motions, g_comma_after_back, "aaa bbb ccc", "cwXX<Esc>wcwYY<Esc>$g;g;g,");
neovim_test!(motions, g_semicolon_no_changes, "hello world", "g;");
neovim_test!(motions, g_comma_at_newest, "hello world", "cwfoo<Esc>g,");

// ─────────────────────────────────────────────────────────────────────────────
// Operator + Less-Common Motions
// ─────────────────────────────────────────────────────────────────────────────

// Operators with + and - motions
neovim_test!(motions, d_plus, "aaa\n  bbb\nccc", "d+");
neovim_test!(motions, d_minus, "aaa\n  bbb\nccc", cursor(2, 0), "d-");
neovim_test!(motions, c_plus, "aaa\n  bbb\nccc", "c+new<Esc>");
neovim_test!(motions, y_plus, "aaa\nbbb\nccc", "y+Gp");

// Operators with ge/gE (word end backward)
neovim_test!(motions, d_ge, "hello world test", cursor(0, 11), "dge");
neovim_test!(motions, c_ge, "hello world test", cursor(0, 11), "cgeX<Esc>");
neovim_test!(motions, d_gE, "hello-world test", cursor(0, 12), "dgE");

// Operators with g_ (last non-blank)
neovim_test!(motions, d_g_underscore, "hello world   ", "dg_");
neovim_test!(motions, y_g_underscore, "hello world   ", "yg_p");

// Operators with _ (first non-blank line motion)
neovim_test!(motions, d_underscore, "  hello world", "d_");
neovim_test!(motions, c_underscore, "  hello world", "c_new<Esc>");

// Operators with display-line motions (gj, gk)
neovim_test!(motions, d_gj, "line one\nline two\nline three", "dgj");
neovim_test!(motions, d_gk, "line one\nline two\nline three", cursor(1, 0), "dgk");
neovim_test!(motions, y_gj, "aaa\nbbb\nccc", "ygjGp");

// Operators with | (go to column)
neovim_test!(motions, d_pipe, "hello world test", "d5|");
neovim_test!(motions, y_pipe, "hello world test", cursor(0, 10), "y5|0p");

// ═══════════════════════════════════════════════════════════════════════════════
// SCROLL + FIRST NON-BLANK (z<CR>, z., z-)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(motions, z_cr_indented, "    hello\n    world", "z\n");
neovim_test!(motions, z_dot_indented, "    hello\n    world", "z.");
neovim_test!(motions, z_minus_indented, "    hello\n    world", "z-");
neovim_test!(motions, z_cr_no_indent, "hello", "z\n");

// ═══════════════════════════════════════════════════════════════════════════════
// g_ WITH COUNT (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// g_ (count=1) on line with trailing whitespace
neovim_test!(motions, g_underscore_trailing_ws, "  abc  \n  def  ", "g_");

// 2g_ moves to last non-blank of next line
neovim_test!(motions, g_underscore_count_2, "  abc  \n  def  \n  ghi  ", "2g_");

// 3g_ moves to last non-blank of line 2
neovim_test!(motions, g_underscore_count_3, "  abc  \n  def  \n  ghi  ", "3g_");

// Large count clamps to last line
neovim_test!(motions, g_underscore_count_clamp, "abc\ndef", "99g_");

// ═══════════════════════════════════════════════════════════════════════════════
// STICKY EOL (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// $ then jj stays at EOL on each line
neovim_test!(motions, dollar_jj_sticky_eol, "abc\nde\nfghij", "$jj");

// ═══════════════════════════════════════════════════════════════════════════════
// h/l WITHOUT WHICHWRAP (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// h at col 0 without whichwrap stays put
neovim_test!(motions, h_no_wrap_at_bol, "abc\ndef", cursor(1, 0), "h");

// l at end of line without whichwrap stays put
neovim_test!(motions, l_no_wrap_at_eol, "abc\ndef", cursor(0, 2), "l");
