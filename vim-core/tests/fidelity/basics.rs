// Basic fidelity tests for vim-core.
//
// These tests compare vim-core output against Neovim oracle.
//
// Test Categories:
// - Basic navigation (h, j, k, l)
// - Line navigation (0, ^, $, g0, g$, gj, gk, |, -, +, _)
// - Word motion (w, b, e)
// - Mode changes (i, a, I, A, o, O, v, V, Ctrl-V, R, Esc)
// - Sticky column behaviour across vertical motions
// - Edge cases: empty buffers, whitespace-only text, unicode, emoji

// ─────────────────────────────────────────────────────────────────────────────
// Basic Navigation (hjkl)
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(no_op, "hello", "");
neovim_test!(cursor_right, "hello", "l");
neovim_test!(cursor_left, "hello", "lh");
neovim_test!(cursor_down, "a\nb", "j");
neovim_test!(cursor_up, "a\nb", "jk");

// NEW: More basic navigation cases
neovim_test!(cursor_right_multiple, "hello world", "lll");
neovim_test!(cursor_left_multiple, "hello", cursor(0, 4), "hhh");
neovim_test!(cursor_down_multiple, "a\nb\nc\nd", "jjj");
neovim_test!(cursor_up_multiple, "a\nb\nc\nd", cursor(3, 0), "kkk");
neovim_test!(cursor_h_at_bol, "hello", "h");
neovim_test!(cursor_l_at_eol, "hello", cursor(0, 4), "l");
neovim_test!(cursor_j_at_last_line, "hello\nworld", cursor(1, 0), "j");
neovim_test!(cursor_k_at_first_line, "hello", "k");
neovim_test!(cursor_with_count_l, "hello world", "5l");
neovim_test!(cursor_with_count_h, "hello world", cursor(0, 6), "5h");
neovim_test!(cursor_with_count_j, "a\nb\nc\nd\ne", "3j");
neovim_test!(cursor_with_count_k, "a\nb\nc\nd\ne", cursor(4, 0), "3k");
neovim_test!(cursor_count_exceeds_l, "hello", "99l");
neovim_test!(cursor_count_exceeds_h, "hello", cursor(0, 4), "99h");
neovim_test!(cursor_unicode_l, "日本語", "l");
neovim_test!(cursor_unicode_h, "日本語", cursor(0, 2), "h");

// ─────────────────────────────────────────────────────────────────────────────
// Line Navigation
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(cursor_eol, "hello", "$");
neovim_test!(cursor_bol, "hello", "$0");

// NEW: More line navigation cases
neovim_test!(cursor_first_nonblank, "  hello", "^");
neovim_test!(cursor_first_nonblank_from_end, "  hello", "$^");
neovim_test!(cursor_eol_multiline, "hello\nworld\ntest", "j$");
neovim_test!(cursor_bol_multiline, "hello\nworld\ntest", cursor(1, 3), "0");
neovim_test!(cursor_gj, "hello world this is a very long line", "gj");
neovim_test!(cursor_gk, "hello world this is a very long line\ntest", cursor(1, 0), "gk");
neovim_test!(cursor_g0, "hello", cursor(0, 3), "g0");
neovim_test!(cursor_g_dollar, "hello", "g$");
neovim_test!(cursor_pipe, "hello world", cursor(0, 6), "3|");
neovim_test!(cursor_pipe_exceeds, "hello", "99|");
neovim_test!(cursor_minus, "hello\n  world", cursor(1, 3), "-");
neovim_test!(cursor_plus, "  hello\nworld", "+");
neovim_test!(cursor_underscore, "  hello", "_");

// ─────────────────────────────────────────────────────────────────────────────
// Word Motion
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(cursor_word, "hello world", "w");

// NEW: More word motion cases
neovim_test!(cursor_word_multiple, "one two three four", "www");
neovim_test!(cursor_word_count, "one two three four", "3w");
neovim_test!(cursor_big_word, "hello-world test", "W");
neovim_test!(cursor_word_back, "hello world", cursor(0, 6), "b");
neovim_test!(cursor_word_end, "hello world", "e");
neovim_test!(cursor_big_word_back, "hello-world test", cursor(0, 12), "B");
neovim_test!(cursor_big_word_end, "hello-world test", "E");
neovim_test!(cursor_ge, "hello world", cursor(0, 6), "ge");
neovim_test!(cursor_gE, "hello-world test", cursor(0, 12), "gE");
neovim_test!(cursor_word_at_eol, "hello world", cursor(0, 10), "w");
neovim_test!(cursor_word_at_last, "hello", cursor(0, 4), "w");
neovim_test!(cursor_b_at_bol, "hello world", "b");
neovim_test!(cursor_word_unicode, "日本語 テスト", "w");
neovim_test!(cursor_word_punctuation, "hello, world! test", "w");

// ─────────────────────────────────────────────────────────────────────────────
// Mode Changes
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(enter_insert, "hello", "i");
neovim_test!(escape_insert, "hello", "i<Esc>");

// NEW: More mode change cases
neovim_test!(enter_append, "hello", "a");
neovim_test!(escape_append, "hello", "a<Esc>");
neovim_test!(enter_insert_bol, "  hello", "I");
neovim_test!(enter_append_eol, "hello", "A");
neovim_test!(enter_visual, "hello", "v");
neovim_test!(escape_visual, "hello", "v<Esc>");
neovim_test!(enter_visual_line, "hello", "V");
neovim_test!(escape_visual_line, "hello", "V<Esc>");
neovim_test!(enter_visual_block, "hello\nworld", "<C-v>");
neovim_test!(escape_visual_block, "hello\nworld", "<C-v><Esc>");
neovim_test!(enter_replace, "hello", "R");
neovim_test!(escape_replace, "hello", "R<Esc>");
neovim_test!(enter_open_below, "hello", "o");
neovim_test!(enter_open_above, "hello", "O");
neovim_test!(mode_double_escape, "hello", "i<Esc><Esc>");
neovim_test!(v_to_V_toggle, "hello", "vV");
neovim_test!(V_to_v_toggle, "hello", "Vv");
neovim_test!(v_to_block_toggle, "hello\nworld", "v<C-v>");

// ─────────────────────────────────────────────────────────────────────────────
// Sticky Column
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(sticky_col_short_line, "12345\n123\n12345", cursor(0, 4), "jj");
neovim_test!(sticky_col_empty_line, "hello\n\nhello", cursor(0, 3), "jj");
neovim_test!(sticky_col_with_dollar, "short\nverylongline\nshort", "$jj");
neovim_test!(sticky_col_clear_on_h, "12345\n123\n12345", cursor(0, 4), "jhj");
neovim_test!(sticky_col_clear_on_l, "12345\n123\n12345", cursor(0, 4), "jlj");
neovim_test!(sticky_col_clear_on_w, "12345\n123\n12345", cursor(0, 4), "jwj");
neovim_test!(sticky_col_up_and_down, "12345\n1\n12345", cursor(0, 4), "jkj");
neovim_test!(sticky_col_count_j, "12345\n1\n12345", cursor(0, 4), "2j");

// ─────────────────────────────────────────────────────────────────────────────
// Edge Cases
// ─────────────────────────────────────────────────────────────────────────────

neovim_test!(basics_empty_buffer, "", "");
neovim_test!(basics_single_char, "a", "");
neovim_test!(single_char_l, "a", "l");
neovim_test!(single_char_h, "a", "h");
neovim_test!(empty_buffer_i, "", "i");
neovim_test!(empty_buffer_a, "", "a");
neovim_test!(basics_newline_only, "\n", "");
neovim_test!(spaces_only, "   ", "");
neovim_test!(tab_navigation, "\thello", "l");
neovim_test!(basics_mixed_whitespace, " \t hello", "w");
neovim_test!(unicode_cjk, "日本語テスト", "lll");
neovim_test!(emoji_navigation, "👍 test 👍", "w");
neovim_test!(basics_combining_chars, "café", "l");
// ZWJ sequences can cause Neovim oracle to hang - skipped
// neovim_test!(zero_width_joiner, "👨‍👩‍👧‍👦 test", "w");

