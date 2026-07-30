// Search fidelity tests (/, ?, n, N, *, #).
//
// Tests for forward/backward search, search repeat, and word search.

// ═══════════════════════════════════════════════════════════════════════════════
// FORWARD SEARCH (/)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, forward_basic, "hello world hello", "/hello<CR>");
neovim_test!(search, forward_second_match, "aaa bbb aaa", "/aaa<CR>n");
neovim_test!(search, forward_no_match, "hello world", "/zzz<CR>");
neovim_test!(search, forward_from_middle, "hello world hello", cursor(0, 6), "/hello<CR>");
neovim_test!(search, forward_multiline, "foo\nbar\nfoo", "/foo<CR>n");
neovim_test!(search, forward_wrapscan, "aaa bbb ccc", cursor(0, 5), "/aaa<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// BACKWARD SEARCH (?)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, backward_basic, "hello world hello", cursor(0, 12), "?hello<CR>");
neovim_test!(search, backward_no_match, "hello world", "?zzz<CR>");
neovim_test!(search, backward_wrapscan, "aaa bbb ccc aaa", "?aaa<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH REPEAT (n, N)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, n_after_forward, "aaa bbb aaa bbb aaa", "/aaa<CR>nn");
neovim_test!(search, N_after_forward, "aaa bbb aaa bbb aaa", "/aaa<CR>nN");
neovim_test!(search, n_after_backward, "aaa bbb aaa", cursor(0, 8), "?aaa<CR>n");
neovim_test!(search, N_after_backward, "aaa bbb aaa", cursor(0, 8), "?aaa<CR>N");

// ═══════════════════════════════════════════════════════════════════════════════
// WORD SEARCH (*, #)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, star_basic, "hello world hello", "*");
neovim_test!(search, star_no_second, "hello world", "*");
neovim_test!(search, hash_basic, "hello world hello", cursor(0, 12), "#");
neovim_test!(search, star_then_n, "foo bar foo baz foo", "*n");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH WITH OPERATORS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, delete_to_search, "hello world foo", "d/foo<CR>");
neovim_test!(search, change_to_search, "hello world foo", "c/foo<CR>X<Esc>");
neovim_test!(search, yank_to_search, "hello world foo", "y/foo<CR>$p");
neovim_test!(search, delete_backward_search, "foo hello world", cursor(0, 10), "d?foo<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// EMPTY SEARCH (repeat last pattern)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, empty_search_repeat, "aaa bbb aaa bbb aaa", "/aaa<CR>//<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH WITH COUNT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, forward_count_2, "aaa bbb aaa bbb aaa", "2/aaa<CR>");
neovim_test!(search, forward_count_3, "aaa bbb aaa bbb aaa", "3/aaa<CR>");
neovim_test!(search, n_with_count, "aaa bbb aaa bbb aaa", "/aaa<CR>2n");
neovim_test!(search, N_with_count, "aaa bbb aaa bbb aaa", cursor(0, 16), "?aaa<CR>2N");
neovim_test!(search, backward_count_2, "aaa bbb aaa bbb aaa", cursor(0, 16), "2?aaa<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH PATTERNS (REGEX)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, regex_dot, "abc aXc", "/a.c<CR>");
neovim_test!(search, regex_star, "aabbb ac", "/ab*<CR>");
neovim_test!(search, regex_bracket_class, "a1b c2d", "/[0-9]<CR>");
neovim_test!(search, regex_word_boundary, "foo foobar", "/\\<foo\\><CR>");
neovim_test!(search, regex_anchor_start, "hello\nworld", "/^world<CR>");
neovim_test!(search, regex_anchor_end, "hello\nworld", "/lo$<CR>");
neovim_test!(search, regex_plus, "aabbb ac", "/ab\\+<CR>");
neovim_test!(search, regex_escaped_dot, "a.b axb", "/a\\.b<CR>");
neovim_test!(search, regex_alternation, "cat dog bird", "/cat\\|dog<CR>");
neovim_test!(search, regex_group, "abcabc", "/\\(abc\\)\\1<CR>");
neovim_test!(search, regex_any_digit, "abc 123 def", "/[0-9]\\+<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// CASE-SENSITIVE / INSENSITIVE SEARCH
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, case_sensitive_default, "Hello hello HELLO", "/Hello<CR>");
neovim_test!(search, case_insensitive_c, "Hello hello HELLO", "/\\chello<CR>");
neovim_test!(search, case_sensitive_C, "Hello hello HELLO", "/\\Chello<CR>");
neovim_test!(search, case_c_then_n, "Hello hello HELLO", "/\\chello<CR>n");
neovim_test!(search, case_c_then_N, "Hello hello HELLO", "/\\chello<CR>N");

// ═══════════════════════════════════════════════════════════════════════════════
// VERY MAGIC / VERY NOMAGIC SEARCH
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, very_magic_basic, "foo(bar)", "/\\v\\(bar\\)<CR>");
neovim_test!(search, very_magic_group, "aabbb", "/\\vab+<CR>");
neovim_test!(search, very_nomagic_literal, "a.b", "/\\Va.b<CR>");
neovim_test!(search, nomagic_literal, "a.b axb", "/\\Ma.b<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH WITH MORE OPERATORS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, delete_backward_to_search, "hello world foo", cursor(0, 10), "d?hello<CR>");
neovim_test!(search, change_backward_search, "foo hello world", cursor(0, 10), "c?hello<CR>X<Esc>");
neovim_test!(search, yank_backward_search, "foo hello world", cursor(0, 10), "y?hello<CR>$p");
neovim_test!(search, delete_to_search_multiline, "hello\nworld\nfoo", "d/foo<CR>");
neovim_test!(search, visual_search_delete, "hello world foo", "v/foo<CR>d");
neovim_test!(search, visual_search_change, "hello world foo", "v/foo<CR>cX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH OFFSET
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, offset_positive, "hello world foo", "/world/+1<CR>");
neovim_test!(search, offset_negative, "hello\nworld\nfoo", "/foo/-1<CR>");
neovim_test!(search, offset_e, "hello world", "/world/e<CR>");
neovim_test!(search, offset_b, "hello world", "/world/b+2<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// STAR/HASH EXTENDED
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, star_word_boundary, "the them there", "*");
neovim_test!(search, star_at_end_of_word, "hello world hello", cursor(0, 4), "*");
neovim_test!(search, hash_first_match, "hello world hello", cursor(0, 12), "#");
neovim_test!(search, hash_then_n, "hello world hello", cursor(0, 12), "#n");
neovim_test!(search, hash_then_N, "hello world hello", cursor(0, 12), "#N");
neovim_test!(search, star_on_special, "foo-bar foo-bar", "*");
neovim_test!(search, star_count_2, "aaa bbb aaa bbb aaa", "2*");
neovim_test!(search, hash_count_2, "aaa bbb aaa bbb aaa", cursor(0, 16), "2#");
neovim_test!(search, g_star_partial, "the them there", "g*");
neovim_test!(search, g_hash_partial, "the them there", cursor(0, 8), "g#");

// ═══════════════════════════════════════════════════════════════════════════════
// EMPTY SEARCH EXTENDED
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, empty_backward_repeat, "aaa bbb aaa", cursor(0, 8), "?aaa<CR>??<CR>");
neovim_test!(search, search_then_star, "hello world hello", "/hello<CR>*");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH WRAPPING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, wrapscan_forward, "aaa bbb ccc", cursor(0, 8), "/aaa<CR>");
neovim_test!(search, wrapscan_backward, "aaa bbb ccc", "?ccc<CR>");
neovim_test!(search, wrapscan_n_cycles, "aaa bbb", "/aaa<CR>nn");
neovim_test!(search, wrapscan_N_cycles, "aaa bbb", "?aaa<CR>NN");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, search_single_char, "a b c a b c", "/a<CR>");
neovim_test!(search, search_at_eol, "hello\nworld", "/hello<CR>");
neovim_test!(search, search_unicode, "日本語 テスト 日本語", "/日本語<CR>");
neovim_test!(search, search_unicode_n, "日本語 テスト 日本語", "/日本語<CR>n");
neovim_test!(search, search_emoji, "hello 👍 world 👍", "/👍<CR>");
neovim_test!(search, search_empty_buffer, "", "/hello<CR>");
neovim_test!(search, search_single_char_buffer, "a", "/a<CR>");
neovim_test!(search, search_cancel_escape, "hello", "/wor<Esc>");
neovim_test!(search, search_backspace, "hello world", "/wor<BS><BS><BS>hello<CR>");
neovim_test!(search, star_single_char_word, "a b c a", "*");
neovim_test!(search, star_on_empty_line, "hello\n\nworld", cursor(1, 0), "*");
neovim_test!(search, search_special_regex_chars, "a(b)c", "/a(b)c<CR>");
neovim_test!(search, search_newline_pattern, "hello world", "/hello\\nworld<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH + DOT REPEAT (n. PATTERN)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(search, n_dot_cw, "foo bar foo baz foo", "/foo<CR>cwX<Esc>n.n.");
neovim_test!(search, n_dot_x, "foo bar foo baz foo", "/foo<CR>xn.n.");
neovim_test!(search, star_n_dot_cw, "hello world hello end hello", "*NcwX<Esc>n.n.");
neovim_test!(search, hash_n_dot_cw, "hello world hello", cursor(0, 12), "#ciwX<Esc>n.");

// ═══════════════════════════════════════════════════════════════════════════════
// % BRACKET MATCHING (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// % skips paren inside double-quoted string
neovim_test!(search, percent_skip_string_paren, "foo(\"(\")bar)", cursor(0, 3), "%");

// % skips } inside line comment
neovim_test!(search, percent_skip_line_comment, "{ // }", "%");

// % normal match on (hello)
neovim_test!(search, percent_basic_paren, "(hello)", "%");

// % skips } inside block comment
neovim_test!(search, percent_skip_block_comment, "{ /* } */ }", "%");

// ═══════════════════════════════════════════════════════════════════════════════
// n/N SEARCH REPEAT (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// n after /foo wraps to first occurrence
neovim_test!(search, n_wraps_to_first, "foo bar foo", "/foo<CR>n");

// N goes backward to previous match
neovim_test!(search, N_reverse_from_second, "foo bar foo", cursor(0, 8), "/foo<CR>N");

// 2n with count skips to Nth match
neovim_test!(search, n_count_two, "x foo x foo x foo", "/foo<CR>2n");

// ═══════════════════════════════════════════════════════════════════════════════
// POSIX REGEX CLASSES IN SUBSTITUTE (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// [[:alpha:]] matches alphabetics
neovim_test!(search, sub_posix_alpha, "a1b2", ":s/[[:alpha:]]/X/g<CR>");

// [^[:digit:]] matches non-digits
neovim_test!(search, sub_posix_digit_neg, "a1b2", ":s/[^[:digit:]]/X/g<CR>");

// [[:space:]] matches whitespace
neovim_test!(search, sub_posix_space, "a b", ":s/[[:space:]]/X/g<CR>");

// Combined POSIX classes [[:upper:][:digit:]]
neovim_test!(search, sub_posix_combined, "aB1c", ":s/[[:upper:][:digit:]]/X/g<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// VIM REGEX CHAR CLASSES IN SUBSTITUTE (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// \i matches identifier chars [0-9A-Za-z_]
neovim_test!(search, sub_vim_ident, "a_1 !", ":s/\\i/X/g<CR>");

// \o matches octal digits [0-7]
neovim_test!(search, sub_vim_octal, "0189", ":s/\\o/X/g<CR>");

// \F matches filename chars without digits
neovim_test!(search, sub_vim_F_no_digit, "a1.b", ":s/\\F/X/g<CR>");

// \K matches keyword chars without digits [A-Za-z_]
neovim_test!(search, sub_vim_K_no_digit, "a1_b", ":s/\\K/X/g<CR>");

// Bug hunt: * with partial word match
neovim_test!(search, star_skips_partial_match, "hello helloworld hello", "*");

// ═══════════════════════════════════════════════════════════════════════════════
// CHAINED SEARCH (/foo/;?bar)
// ═══════════════════════════════════════════════════════════════════════════════

// Forward then backward: find "bar", then backward for "foo"
neovim_test!(search, chained_forward_backward, "foo hello bar world", "/bar/;?foo<CR>");

// Forward then forward: find "bar", then forward for "world"
neovim_test!(search, chained_forward_forward, "foo bar world end", "/bar/;/world<CR>");

// Chained search then n repeats last segment's pattern
neovim_test!(search, chained_then_n, "foo bar foo bar foo", "/bar/;/foo<CR>n");

// Backward then forward chain
neovim_test!(search, chained_backward_forward, "foo bar baz end", cursor(0, 12), "?bar?;/baz<CR>");

// Three-segment chain
neovim_test!(search, chained_three_segments, "aaa bbb ccc ddd eee", "/bbb/;/ccc/;/ddd<CR>");

// Chain with no trailing delimiter on first pattern
neovim_test!(search, chained_no_trailing_delim, "foo bar baz", "/bar;?foo<CR>");

// Chain where intermediate segment fails — cursor should stay put
neovim_test!(search, chained_intermediate_fails, "foo hello world", "/foo/;?zzz<CR>");
