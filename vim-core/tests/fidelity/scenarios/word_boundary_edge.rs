// Word Boundary Edge Cases fidelity tests.
//
// Tests for word motions (w, e, b, W, E, B, ge, gE) on tricky content:
// punctuation clusters, mixed whitespace, unicode boundaries, empty lines,
// single-char words, and special characters.

// ═══════════════════════════════════════════════════════════════════════════════
// PUNCTUATION CLUSTERS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, w_single_punct, "a.b", "w");
neovim_test!(scenarios, w_double_punct, "a..b", "w");
neovim_test!(scenarios, w_triple_punct, "a...b", "w");
neovim_test!(scenarios, w_punct_only_line, "...", "w");
neovim_test!(scenarios, wb_w_mixed_punct, "a!@#b", "w");
neovim_test!(scenarios, w_comma_separated, "a,b,c", "w");
neovim_test!(scenarios, w_semicolon_separated, "a;b;c", "w");
neovim_test!(scenarios, w_equals_operator, "a==b", "w");
neovim_test!(scenarios, w_arrow_operator, "a->b", "w");
neovim_test!(scenarios, w_double_colon, "a::b", "w");
neovim_test!(scenarios, w_hash_bang, "#!", "w");
neovim_test!(scenarios, w_forward_slash, "a/b/c", "w");
neovim_test!(scenarios, w_backslash, "a\\b\\c", "w");

neovim_test!(scenarios, e_single_punct, "a.b", "e");
neovim_test!(scenarios, e_double_punct, "a..b", "e");
neovim_test!(scenarios, e_mixed_punct, "a!@#b", "e");
neovim_test!(scenarios, e_comma_separated, "a,b,c", "e");

neovim_test!(scenarios, b_single_punct, "a.b", cursor(0, 2), "b");
neovim_test!(scenarios, b_double_punct, "a..b", cursor(0, 3), "b");
neovim_test!(scenarios, b_mixed_punct, "a!@#b", cursor(0, 4), "b");

// ═══════════════════════════════════════════════════════════════════════════════
// WORD VS WORD (w vs W, e vs E, b vs B)
// ═══════════════════════════════════════════════════════════════════════════════

// w stops at punctuation boundaries, W doesn't
neovim_test!(scenarios, w_vs_W_dotted, "foo.bar baz", "w");
neovim_test!(scenarios, W_vs_w_dotted, "foo.bar baz", "W");
neovim_test!(scenarios, w_vs_W_hyphen, "foo-bar baz", "w");
neovim_test!(scenarios, W_vs_w_hyphen, "foo-bar baz", "W");
neovim_test!(scenarios, w_vs_W_underscore, "foo_bar baz", "w");
neovim_test!(scenarios, W_vs_w_underscore, "foo_bar baz", "W");

neovim_test!(scenarios, e_vs_E_dotted, "foo.bar baz", "e");
neovim_test!(scenarios, E_vs_e_dotted, "foo.bar baz", "E");

neovim_test!(scenarios, b_vs_B_dotted, "foo.bar baz", cursor(0, 8), "b");
neovim_test!(scenarios, B_vs_b_dotted, "foo.bar baz", cursor(0, 8), "B");

neovim_test!(scenarios, ge_vs_gE_dotted, "foo.bar baz", cursor(0, 8), "ge");
neovim_test!(scenarios, gE_vs_ge_dotted, "foo.bar baz", cursor(0, 8), "gE");

// ═══════════════════════════════════════════════════════════════════════════════
// CONSECUTIVE WHITESPACE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, w_double_space, "hello  world", "w");
neovim_test!(scenarios, w_triple_space, "hello   world", "w");
neovim_test!(scenarios, w_many_spaces, "hello      world", "w");
neovim_test!(scenarios, w_tab_space_mix, "hello \t world", "w");
neovim_test!(scenarios, w_newline_space, "hello\n world", "w");

neovim_test!(scenarios, b_double_space, "hello  world", cursor(0, 7), "b");
neovim_test!(scenarios, b_triple_space, "hello   world", cursor(0, 8), "b");
neovim_test!(scenarios, b_tab_space_mix, "hello \t world", cursor(0, 9), "b");

neovim_test!(scenarios, e_double_space, "hello  world", "e");
neovim_test!(scenarios, e_triple_space, "hello   world", "e");

// ═══════════════════════════════════════════════════════════════════════════════
// SINGLE CHARACTER WORDS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, w_single_chars, "a b c d", "w");
neovim_test!(scenarios, w_single_chars_2, "a b c d", "ww");
neovim_test!(scenarios, w_single_chars_3, "a b c d", "www");
neovim_test!(scenarios, e_single_chars, "a b c d", "e");
neovim_test!(scenarios, b_single_chars, "a b c d", cursor(0, 6), "b");
neovim_test!(scenarios, b_single_chars_2, "a b c d", cursor(0, 6), "bb");

neovim_test!(scenarios, dw_single_chars, "a b c d", "dw");
neovim_test!(scenarios, de_single_chars, "a b c d", "de");
neovim_test!(scenarios, cw_single_chars, "a b c d", "cwX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CROSS-LINE WORD MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, w_end_of_line, "hello\nworld", cursor(0, 4), "w");
neovim_test!(scenarios, w_end_of_line_space, "hello \nworld", cursor(0, 5), "w");
neovim_test!(scenarios, w_across_empty_line, "hello\n\nworld", cursor(0, 4), "w");
neovim_test!(scenarios, w_across_multiple_empty, "hello\n\n\nworld", cursor(0, 4), "w");
neovim_test!(scenarios, w_indented_next_line, "hello\n  world", cursor(0, 4), "w");

neovim_test!(scenarios, b_start_of_line, "hello\nworld", cursor(1, 0), "b");
neovim_test!(scenarios, b_across_empty_line, "hello\n\nworld", cursor(2, 0), "b");
neovim_test!(scenarios, b_indented_prev_line, "hello\n  world", cursor(1, 2), "b");

neovim_test!(scenarios, e_end_of_line, "hello\nworld", "e");
neovim_test!(scenarios, e_end_then_next, "hello\nworld", "ee");
neovim_test!(scenarios, e_across_empty_line, "hello\n\nworld", "ee");

neovim_test!(scenarios, ge_start_of_line, "hello\nworld", cursor(1, 0), "ge");
neovim_test!(scenarios, ge_across_empty_line, "hello\n\nworld", cursor(2, 0), "ge");

// ═══════════════════════════════════════════════════════════════════════════════
// UNICODE WORD BOUNDARIES
// ═══════════════════════════════════════════════════════════════════════════════

// CJK characters — each is its own word
neovim_test!(scenarios, w_cjk, "日本語", "w");
neovim_test!(scenarios, w_cjk_2, "日本語", "ww");
neovim_test!(scenarios, e_cjk, "日本語", "e");
neovim_test!(scenarios, b_cjk, "日本語", cursor(0, 2), "b");

// CJK mixed with ASCII
neovim_test!(scenarios, w_cjk_ascii, "hello日本語world", "w");
neovim_test!(scenarios, w_cjk_ascii_2, "hello日本語world", "ww");
neovim_test!(scenarios, w_cjk_space_ascii, "日本語 hello", "w");
neovim_test!(scenarios, b_cjk_ascii, "hello日本語world", cursor(0, 14), "b");

// Emoji
neovim_test!(scenarios, w_emoji, "👍 hello", "w");
neovim_test!(scenarios, w_emoji_cluster, "👨‍👩‍👧 hello", "w");
neovim_test!(scenarios, e_emoji, "👍 hello", "e");
neovim_test!(scenarios, b_emoji, "hello 👍", cursor(0, 6), "b");

// Accented characters
neovim_test!(scenarios, w_accented, "café résumé", "w");
neovim_test!(scenarios, e_accented, "café résumé", "e");
neovim_test!(scenarios, b_accented, "café résumé", cursor(0, 5), "b");

// Mixed scripts
neovim_test!(scenarios, w_mixed_scripts, "hello мир 世界", "w");
neovim_test!(scenarios, w_mixed_scripts_2, "hello мир 世界", "ww");
neovim_test!(scenarios, e_mixed_scripts, "hello мир 世界", "e");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATORS WITH WORD BOUNDARIES
// ═══════════════════════════════════════════════════════════════════════════════

// dw on various boundary types
neovim_test!(scenarios, dw_at_punct, "foo.bar", "dw");
neovim_test!(scenarios, dw_at_space, "foo bar", cursor(0, 3), "dw");
neovim_test!(scenarios, dw_at_last_word, "foo bar", cursor(0, 4), "dw");
neovim_test!(scenarios, dw_at_eol_cross, "foo\nbar", cursor(0, 2), "dw");

// cw edge case: cw at end of word acts like ce
neovim_test!(scenarios, cw_at_word_end_edge, "hello world", cursor(0, 4), "cwX<Esc>");
neovim_test!(scenarios, cw_at_word_start, "hello world", "cwX<Esc>");
neovim_test!(scenarios, cw_at_space, "hello world", cursor(0, 5), "cwX<Esc>");
neovim_test!(scenarios, ce_at_word_end, "hello world", cursor(0, 4), "ceX<Esc>");
neovim_test!(scenarios, ce_at_word_start, "hello world", "ceX<Esc>");

// de vs dw difference
neovim_test!(scenarios, dw_includes_trailing_space, "hello world test", "dw");
neovim_test!(scenarios, de_excludes_trailing_space, "hello world test", "de");

// ═══════════════════════════════════════════════════════════════════════════════
// EMPTY LINES IN WORD MOTION
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, w_from_empty_line, "\nhello", "w");
neovim_test!(scenarios, w_to_empty_line, "hello\n\nworld", "w");
neovim_test!(scenarios, b_from_empty_line, "hello\n", cursor(1, 0), "b");
neovim_test!(scenarios, b_to_empty_line, "hello\n\nworld", cursor(2, 0), "b");
neovim_test!(scenarios, e_from_empty_line, "\nhello", "e");
neovim_test!(scenarios, dw_empty_line, "hello\n\nworld", cursor(1, 0), "dw");
neovim_test!(scenarios, dd_empty_line_in_context, "hello\n\nworld", cursor(1, 0), "dd");

// ═══════════════════════════════════════════════════════════════════════════════
// PROGRAMMING LANGUAGE PATTERNS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, w_function_call, "foo(bar)", "w");
neovim_test!(scenarios, w_method_chain, "foo.bar.baz", "w");
neovim_test!(scenarios, w_array_access, "arr[idx]", "w");
neovim_test!(scenarios, w_template, "Vec<String>", "w");
neovim_test!(scenarios, w_assignment, "x = 42", "w");
neovim_test!(scenarios, w_comparison, "x == y", "w");
neovim_test!(scenarios, w_arrow_fn, "x => y", "w");
neovim_test!(scenarios, w_namespace, "std::io::Read", "w");
neovim_test!(scenarios, w_url_path, "http://foo/bar", "w");
neovim_test!(scenarios, w_file_path, "/home/user/file.txt", "w");
neovim_test!(scenarios, w_snake_case, "hello_world_test", "w");
neovim_test!(scenarios, w_camel_case, "helloWorldTest", "w");
neovim_test!(scenarios, w_const_case, "HELLO_WORLD", "w");
neovim_test!(scenarios, w_kebab_case, "hello-world-test", "w");

neovim_test!(scenarios, W_function_call, "foo(bar)", "W");
neovim_test!(scenarios, W_method_chain, "foo.bar.baz", "W");
neovim_test!(scenarios, W_namespace, "std::io::Read", "W");
neovim_test!(scenarios, W_snake_case, "hello_world_test", "W");

// ═══════════════════════════════════════════════════════════════════════════════
// WORD MOTIONS WITH COUNTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, w_count_2, "one two three four", "2w");
neovim_test!(scenarios, w_count_3, "one two three four", "3w");
neovim_test!(scenarios, w_count_10, "a b c d e f g h i j k", "10w");
neovim_test!(scenarios, w_count_past_end, "one two three", "100w");
neovim_test!(scenarios, e_count_2, "one two three four", "2e");
neovim_test!(scenarios, e_count_3, "one two three four", "3e");
neovim_test!(scenarios, b_count_2, "one two three four", cursor(0, 14), "2b");
neovim_test!(scenarios, b_count_3, "one two three four", cursor(0, 14), "3b");
neovim_test!(scenarios, b_count_past_start, "one two three", cursor(0, 8), "100b");

neovim_test!(scenarios, dw_count_2, "one two three four", "d2w");
neovim_test!(scenarios, dw_count_3, "one two three four", "d3w");
neovim_test!(scenarios, cw_count_2, "one two three four", "c2wX<Esc>");
neovim_test!(scenarios, yw_count_2, "one two three four", "y2w$p");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES: LAST/FIRST POSITION
// ═══════════════════════════════════════════════════════════════════════════════

// w/e at end of buffer (on last char of last word)
neovim_test!(scenarios, w_at_last_char, "hello", cursor(0, 4), "w");
neovim_test!(scenarios, e_at_last_char, "hello", cursor(0, 4), "e");
neovim_test!(scenarios, w_at_last_word, "hello world", cursor(0, 6), "w");
neovim_test!(scenarios, e_at_last_word_end, "hello world", cursor(0, 10), "e");

// b/ge at start of buffer
neovim_test!(scenarios, b_at_first_char, "hello", "b");
neovim_test!(scenarios, ge_at_first_char, "hello", "ge");
neovim_test!(scenarios, b_at_first_word, "hello world", cursor(0, 6), "b");
neovim_test!(scenarios, ge_at_second_word_start, "hello world", cursor(0, 6), "ge");
