// Text Object fidelity tests for vim-core.
//
// These tests compare vim-core text object output against Neovim oracle.
// 100+ tests covering all text object categories per phase-7-textobjects.md.
//
// Test Categories:
// - Word objects (iw, aw, iW, aW): 25 tests
// - Bracket objects (i(, a(, etc): 25 tests
// - Quote objects (i", a", etc): 20 tests
// - Paragraph objects (ip, ap): 15 tests
// - Sentence objects (is, as): 10 tests
// - Tag objects (it, at): 10 tests

// ═══════════════════════════════════════════════════════════════════════════════
// WORD OBJECTS (iw, aw, iW, aW)
// ═══════════════════════════════════════════════════════════════════════════════

// Inner word (iw) - select word under cursor
neovim_test!(textobjects, diw_basic, "hello world", "diw");
neovim_test!(textobjects, diw_middle, "hello world foo", cursor(0, 6), "diw");
neovim_test!(textobjects, diw_cursor_on_space, "hello world", cursor(0, 5), "diw");
neovim_test!(textobjects, diw_single_char, "a b c", cursor(0, 2), "diw");
neovim_test!(textobjects, diw_at_eol, "hello", cursor(0, 4), "diw");
neovim_test!(textobjects, yiw_yank, "hello world", "yiwP");
neovim_test!(textobjects, ciw_change, "hello world", cursor(0, 6), "ciwfoo<Esc>");
neovim_test!(textobjects, viw_select, "hello world", "viwd");

// Around word (aw) - select word + trailing whitespace
neovim_test!(textobjects, daw_basic, "hello world", "daw");
neovim_test!(textobjects, daw_middle, "one two three", cursor(0, 4), "daw");
neovim_test!(textobjects, daw_last_word, "hello world", cursor(0, 6), "daw");
neovim_test!(textobjects, daw_single_word, "hello", "daw");
neovim_test!(textobjects, yaw_yank, "hello world", "yawP");
neovim_test!(textobjects, caw_change, "hello world", "cawfoo<Esc>");

// Inner WORD (iW) - treats punctuation as part of word
neovim_test!(textobjects, diW_with_punct, "foo.bar baz", "diW");
neovim_test!(textobjects, diW_middle, "one foo.bar two", cursor(0, 4), "diW");
neovim_test!(textobjects, yiW_yank, "foo.bar baz", "yiWP");

// Around WORD (aW)
neovim_test!(textobjects, daW_with_punct, "foo.bar baz", "daW");
neovim_test!(textobjects, daW_middle, "one foo.bar two", cursor(0, 4), "daW");

// Word objects with counts
neovim_test!(textobjects, d2iw_count, "one two three", "d2iw");
neovim_test!(textobjects, d2aw_count, "one two three", "d2aw");

// Edge cases
neovim_test!(textobjects, diw_empty, "", "diw");
neovim_test!(textobjects, diw_whitespace_only, "   ", cursor(0, 1), "diw");
neovim_test!(textobjects, diw_unicode, "héllo wörld", "diw");

// NEW: Word object edge cases
neovim_test!(textobjects, diw_cjk, "日本語 hello", "diw");
neovim_test!(textobjects, daw_cjk, "日本語 hello", "daw");
neovim_test!(textobjects, diw_emoji, "👍 test", "diw");
neovim_test!(textobjects, diw_tabs, "hello\tworld", "diw");
neovim_test!(textobjects, daw_tabs, "hello\tworld", "daw");
neovim_test!(textobjects, diw_multiple_spaces, "hello    world", "diw");
neovim_test!(textobjects, daw_multiple_spaces, "hello    world", cursor(0, 7), "daw");
neovim_test!(textobjects, diw_first_char, "hello world", cursor(0, 0), "diw");
neovim_test!(textobjects, daw_first_char, "hello world", cursor(0, 0), "daw");
neovim_test!(textobjects, diw_last_char, "hello world", cursor(0, 10), "diw");
neovim_test!(textobjects, daw_last_char, "hello world", cursor(0, 10), "daw");
neovim_test!(textobjects, diw_punctuation, "hello, world", cursor(0, 5), "diw");
neovim_test!(textobjects, daw_punctuation, "hello, world", cursor(0, 5), "daw");
neovim_test!(textobjects, diW_url, "http://example.com test", "diW");
neovim_test!(textobjects, daW_url, "http://example.com test", "daW");
neovim_test!(textobjects, viw_visual, "hello world", "viwld");
neovim_test!(textobjects, vaw_visual, "hello world", "vawld");
neovim_test!(textobjects, diw_count_3, "one two three four", "3diw");
neovim_test!(textobjects, daw_count_3, "one two three four", "3daw");

// ═══════════════════════════════════════════════════════════════════════════════
// BRACKET OBJECTS - PARENTHESES
// ═══════════════════════════════════════════════════════════════════════════════

// Inner paren (i(, ib)
neovim_test!(textobjects, di_paren_basic, "(hello)", cursor(0, 1), "di(");
neovim_test!(textobjects, dib_basic, "(hello)", cursor(0, 1), "dib");
neovim_test!(textobjects, di_paren_nested, "(a(b)c)", cursor(0, 3), "di(");
neovim_test!(textobjects, di_paren_outer, "(a(b)c)", cursor(0, 1), "di(");
neovim_test!(textobjects, yi_paren_yank, "(hello)", cursor(0, 1), "yi(P");
neovim_test!(textobjects, ci_paren_change, "(hello)", cursor(0, 1), "ci(new<Esc>");

// Around paren (a(, ab)
neovim_test!(textobjects, da_paren_basic, "(hello)", cursor(0, 1), "da(");
neovim_test!(textobjects, dab_basic, "(hello)", cursor(0, 1), "dab");
neovim_test!(textobjects, da_paren_nested, "(a(b)c)", cursor(0, 3), "da(");
neovim_test!(textobjects, ya_paren_yank, "(hello)", cursor(0, 1), "ya(P");

// Cursor on bracket
neovim_test!(textobjects, di_paren_cursor_on_open, "(hello)", cursor(0, 0), "di(");
neovim_test!(textobjects, di_paren_cursor_on_close, "(hello)", cursor(0, 6), "di(");

// NEW: Paren edge cases
neovim_test!(textobjects, di_paren_empty, "()", cursor(0, 0), "di(");
neovim_test!(textobjects, da_paren_empty, "()", cursor(0, 0), "da(");
neovim_test!(textobjects, di_paren_whitespace, "(  )", cursor(0, 1), "di(");
neovim_test!(textobjects, di_paren_newlines, "(\nhello\n)", cursor(1, 0), "di(");
neovim_test!(textobjects, da_paren_newlines, "(\nhello\n)", cursor(1, 0), "da(");
neovim_test!(textobjects, di_paren_deeply_nested, "(a(b(c(d))))", cursor(0, 6), "di(");
neovim_test!(textobjects, da_paren_deeply_nested, "(a(b(c(d))))", cursor(0, 6), "da(");
neovim_test!(textobjects, di_paren_unicode, "(日本語)", cursor(0, 1), "di(");
neovim_test!(textobjects, vi_paren_select, "(hello)", cursor(0, 1), "vi(d");
neovim_test!(textobjects, va_paren_select, "(hello)", cursor(0, 1), "va(d");
neovim_test!(textobjects, di_paren_before, "hello (world)", cursor(0, 0), "di(");
neovim_test!(textobjects, di_paren_multiple, "(a) (b) (c)", cursor(0, 5), "di(");
neovim_test!(textobjects, di_paren_with_counts, "((inner))", cursor(0, 2), "2di(");

// ═══════════════════════════════════════════════════════════════════════════════
// BRACKET OBJECTS - BRACES
// ═══════════════════════════════════════════════════════════════════════════════

// Inner brace (i{, iB)
neovim_test!(textobjects, di_brace_basic, "{hello}", cursor(0, 1), "di{");
neovim_test!(textobjects, diB_basic, "{hello}", cursor(0, 1), "diB");
neovim_test!(textobjects, di_brace_nested, "{a{b}c}", cursor(0, 3), "di{");
neovim_test!(textobjects, ci_brace_change, "{hello}", cursor(0, 1), "ci{new<Esc>");

// Around brace (a{, aB)
neovim_test!(textobjects, da_brace_basic, "{hello}", cursor(0, 1), "da{");
neovim_test!(textobjects, daB_basic, "{hello}", cursor(0, 1), "daB");
neovim_test!(textobjects, da_brace_multiline, "{\nfoo\n}", cursor(1, 0), "da{");

// NEW: Brace edge cases
neovim_test!(textobjects, di_brace_empty, "{}", cursor(0, 0), "di{");
neovim_test!(textobjects, da_brace_empty, "{}", cursor(0, 0), "da{");
neovim_test!(textobjects, di_brace_code, "fn foo() { bar(); }", cursor(0, 12), "di{");
neovim_test!(textobjects, da_brace_code, "fn foo() { bar(); }", cursor(0, 12), "da{");
neovim_test!(textobjects, di_brace_multiline_code, "{\n  let x = 1;\n  let y = 2;\n}", cursor(1, 0), "di{");
neovim_test!(textobjects, vi_brace_select, "{hello}", cursor(0, 1), "vi{d");
neovim_test!(textobjects, va_brace_select, "{hello}", cursor(0, 1), "va{d");
neovim_test!(textobjects, di_brace_deeply_nested, "{a{b{c{d}}}}", cursor(0, 6), "di{");
neovim_test!(textobjects, di_brace_unicode, "{日本語}", cursor(0, 1), "di{");

// ═══════════════════════════════════════════════════════════════════════════════
// BRACKET OBJECTS - SQUARE BRACKETS
// ═══════════════════════════════════════════════════════════════════════════════

// Inner bracket (i[)
neovim_test!(textobjects, di_bracket_basic, "[hello]", cursor(0, 1), "di[");
neovim_test!(textobjects, di_bracket_nested, "[a[b]c]", cursor(0, 3), "di[");
neovim_test!(textobjects, ci_bracket_change, "[hello]", cursor(0, 1), "ci[new<Esc>");

// Around bracket (a[)
neovim_test!(textobjects, da_bracket_basic, "[hello]", cursor(0, 1), "da[");
neovim_test!(textobjects, ya_bracket_yank, "[hello]", cursor(0, 1), "ya[P");

// NEW: Bracket edge cases
neovim_test!(textobjects, di_bracket_empty, "[]", cursor(0, 0), "di[");
neovim_test!(textobjects, da_bracket_empty, "[]", cursor(0, 0), "da[");
neovim_test!(textobjects, di_bracket_array, "[1, 2, 3]", cursor(0, 3), "di[");
neovim_test!(textobjects, da_bracket_array, "[1, 2, 3]", cursor(0, 3), "da[");
neovim_test!(textobjects, di_bracket_multiline, "[\n  item1,\n  item2\n]", cursor(1, 2), "di[");
neovim_test!(textobjects, vi_bracket_select, "[hello]", cursor(0, 1), "vi[d");
neovim_test!(textobjects, va_bracket_select, "[hello]", cursor(0, 1), "va[d");
neovim_test!(textobjects, di_bracket_unicode, "[日本語]", cursor(0, 1), "di[");
neovim_test!(textobjects, di_bracket_nested_deep, "[a[b[c]]]", cursor(0, 4), "di[");

// ═══════════════════════════════════════════════════════════════════════════════
// BRACKET OBJECTS - ANGLE BRACKETS
// ═══════════════════════════════════════════════════════════════════════════════

// Inner angle (i<)
// Note: Use <lt> for literal '<' and <gt> for literal '>' to avoid ambiguity with <Esc> etc.
neovim_test!(textobjects, di_angle_basic, "<hello>", cursor(0, 1), "di<lt>");
neovim_test!(textobjects, di_angle_nested, "<a<b>c>", cursor(0, 3), "di<lt>");
neovim_test!(textobjects, ci_angle_change, "<hello>", cursor(0, 1), "ci<lt>new<Esc>");

// Around angle (a<)
neovim_test!(textobjects, da_angle_basic, "<hello>", cursor(0, 1), "da<lt>");
neovim_test!(textobjects, ya_angle_yank, "<hello>", cursor(0, 1), "ya<lt>P");

// NEW: Angle bracket edge cases
neovim_test!(textobjects, di_angle_empty, "<>", cursor(0, 0), "di<lt>");
neovim_test!(textobjects, da_angle_empty, "<>", cursor(0, 0), "da<lt>");
neovim_test!(textobjects, di_angle_generics, "Vec<String>", cursor(0, 5), "di<lt>");
neovim_test!(textobjects, da_angle_generics, "Vec<String>", cursor(0, 5), "da<lt>");
neovim_test!(textobjects, vi_angle_select, "<hello>", cursor(0, 1), "vi<lt>d");
neovim_test!(textobjects, va_angle_select, "<hello>", cursor(0, 1), "va<lt>d");
neovim_test!(textobjects, di_angle_unicode, "<日本語>", cursor(0, 1), "di<lt>");
neovim_test!(textobjects, di_angle_nested_deep, "<a<b<c>>>", cursor(0, 4), "di<lt>");

// ═══════════════════════════════════════════════════════════════════════════════
// QUOTE OBJECTS - DOUBLE QUOTES
// ═══════════════════════════════════════════════════════════════════════════════

// Inner double quote (i")
neovim_test!(textobjects, di_dquote_basic, "\"hello\"", cursor(0, 1), "di\"");
neovim_test!(textobjects, yi_dquote_yank, "\"hello\"", cursor(0, 1), "yi\"P");
neovim_test!(textobjects, ci_dquote_change, "\"hello\"", cursor(0, 1), "ci\"new<Esc>");
neovim_test!(textobjects, di_dquote_cursor_on_quote, "\"hello\"", cursor(0, 0), "di\"");

// Around double quote (a")
neovim_test!(textobjects, da_dquote_basic, "\"hello\"", cursor(0, 1), "da\"");
neovim_test!(textobjects, ya_dquote_yank, "\"hello\"", cursor(0, 1), "ya\"P");

// Multiple pairs on line
neovim_test!(textobjects, di_dquote_second_pair, "\"a\" \"b\"", cursor(0, 5), "di\"");

// Escaped quotes
neovim_test!(textobjects, di_dquote_escaped, "\"hel\\\"lo\"", cursor(0, 1), "di\"");

// NEW: Double quote edge cases
neovim_test!(textobjects, di_dquote_empty, "\"\"", cursor(0, 0), "di\"");
neovim_test!(textobjects, da_dquote_empty, "\"\"", cursor(0, 0), "da\"");
neovim_test!(textobjects, di_dquote_spaces, "\"hello world\"", cursor(0, 1), "di\"");
neovim_test!(textobjects, di_dquote_unicode, "\"日本語\"", cursor(0, 1), "di\"");
neovim_test!(textobjects, vi_dquote_select, "\"hello\"", cursor(0, 1), "vi\"d");
neovim_test!(textobjects, va_dquote_select, "\"hello\"", cursor(0, 1), "va\"d");
neovim_test!(textobjects, di_dquote_before, "hello \"world\"", cursor(0, 0), "di\"");
neovim_test!(textobjects, di_dquote_triple, "\"a\" \"b\" \"c\"", cursor(0, 9), "di\"");
neovim_test!(textobjects, di_dquote_newline_between, "\"hello\nworld\"", cursor(0, 1), "di\"");

// ═══════════════════════════════════════════════════════════════════════════════
// QUOTE OBJECTS - SINGLE QUOTES
// ═══════════════════════════════════════════════════════════════════════════════

// Inner single quote (i')
neovim_test!(textobjects, di_squote_basic, "'hello'", cursor(0, 1), "di'");
neovim_test!(textobjects, ci_squote_change, "'hello'", cursor(0, 1), "ci'new<Esc>");

// Around single quote (a')
neovim_test!(textobjects, da_squote_basic, "'hello'", cursor(0, 1), "da'");

// NEW: Single quote edge cases
neovim_test!(textobjects, di_squote_empty, "''", cursor(0, 0), "di'");
neovim_test!(textobjects, da_squote_empty, "''", cursor(0, 0), "da'");
neovim_test!(textobjects, di_squote_spaces, "'hello world'", cursor(0, 1), "di'");
neovim_test!(textobjects, di_squote_unicode, "'日本語'", cursor(0, 1), "di'");
neovim_test!(textobjects, vi_squote_select, "'hello'", cursor(0, 1), "vi'd");
neovim_test!(textobjects, va_squote_select, "'hello'", cursor(0, 1), "va'd");
neovim_test!(textobjects, di_squote_escaped, "'hel\\'lo'", cursor(0, 1), "di'");
neovim_test!(textobjects, di_squote_multiple, "'a' 'b' 'c'", cursor(0, 5), "di'");
neovim_test!(textobjects, yi_squote_yank, "'hello'", cursor(0, 1), "yi'P");
neovim_test!(textobjects, ya_squote_yank, "'hello'", cursor(0, 1), "ya'P");

// ═══════════════════════════════════════════════════════════════════════════════
// QUOTE OBJECTS - BACKTICKS
// ═══════════════════════════════════════════════════════════════════════════════

// Inner backtick (i`)
neovim_test!(textobjects, di_backtick_basic, "`hello`", cursor(0, 1), "di`");
neovim_test!(textobjects, ci_backtick_change, "`hello`", cursor(0, 1), "ci`new<Esc>");

// Around backtick (a`)
neovim_test!(textobjects, da_backtick_basic, "`hello`", cursor(0, 1), "da`");

// NEW: Backtick edge cases
neovim_test!(textobjects, di_backtick_empty, "``", cursor(0, 0), "di`");
neovim_test!(textobjects, da_backtick_empty, "``", cursor(0, 0), "da`");
neovim_test!(textobjects, di_backtick_code, "`let x = 1;`", cursor(0, 3), "di`");
neovim_test!(textobjects, di_backtick_unicode, "`日本語`", cursor(0, 1), "di`");
neovim_test!(textobjects, vi_backtick_select, "`hello`", cursor(0, 1), "vi`d");
neovim_test!(textobjects, va_backtick_select, "`hello`", cursor(0, 1), "va`d");
neovim_test!(textobjects, di_backtick_multiple, "`a` `b` `c`", cursor(0, 5), "di`");
neovim_test!(textobjects, yi_backtick_yank, "`hello`", cursor(0, 1), "yi`P");
neovim_test!(textobjects, ya_backtick_yank, "`hello`", cursor(0, 1), "ya`P");

// ═══════════════════════════════════════════════════════════════════════════════
// PARAGRAPH OBJECTS (ip, ap)
// ═══════════════════════════════════════════════════════════════════════════════

// Inner paragraph (ip)
neovim_test!(textobjects, dip_single, "hello\nworld", "dip");
neovim_test!(textobjects, dip_with_blank, "para1\n\npara2", "dip");
neovim_test!(textobjects, yip_yank, "hello\nworld", "yipP");
neovim_test!(textobjects, cip_change, "hello\nworld", "cipnew<Esc>");
neovim_test!(textobjects, vip_select, "para1\n\npara2", "vipd");

// Around paragraph (ap)
neovim_test!(textobjects, dap_basic, "para1\n\npara2", "dap");
neovim_test!(textobjects, dap_includes_trailing, "para1\n\npara2\n\npara3", cursor(0, 0), "dap");
neovim_test!(textobjects, yap_yank, "para1\n\npara2", "yapP");

// Cursor on blank line
neovim_test!(textobjects, dip_cursor_on_blank, "para1\n\npara2", cursor(1, 0), "dip");
neovim_test!(textobjects, dap_cursor_on_blank, "para1\n\npara2", cursor(1, 0), "dap");

// Multiple blank lines
neovim_test!(textobjects, dip_multi_blank, "para1\n\n\n\npara2", "dip");
neovim_test!(textobjects, dap_multi_blank, "para1\n\n\n\npara2", "dap");

// Edge cases
neovim_test!(textobjects, dip_single_line, "hello", "dip");
neovim_test!(textobjects, dap_single_line, "hello", "dap");

// NEW: Paragraph edge cases
neovim_test!(textobjects, dip_empty_buffer, "", "dip");
neovim_test!(textobjects, dap_empty_buffer, "", "dap");
neovim_test!(textobjects, dip_whitespace_only, "   \n   \n", cursor(0, 0), "dip");
neovim_test!(textobjects, dip_last_paragraph, "para1\n\npara2", cursor(2, 0), "dip");
neovim_test!(textobjects, dap_last_paragraph, "para1\n\npara2", cursor(2, 0), "dap");
neovim_test!(textobjects, dip_unicode, "日本語\n\nテスト", "dip");
neovim_test!(textobjects, vip_visual, "para1\n\npara2", "vipd");
neovim_test!(textobjects, vap_visual, "para1\n\npara2", "vapd");
neovim_test!(textobjects, dip_middle_line, "line1\nline2\nline3\n\npara2", cursor(1, 0), "dip");
neovim_test!(textobjects, dap_middle_line, "line1\nline2\nline3\n\npara2", cursor(1, 0), "dap");
neovim_test!(textobjects, cip_change_multi, "line1\nline2\n\nline3", "cipnew<Esc>");
neovim_test!(textobjects, cap_change, "line1\nline2\n\nline3", "capnew<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// SENTENCE OBJECTS (is, as)
// ═══════════════════════════════════════════════════════════════════════════════

// Inner sentence (is)
neovim_test!(textobjects, dis_basic, "Hello. World.", "dis");
neovim_test!(textobjects, dis_exclamation, "Hello! World.", "dis");
neovim_test!(textobjects, dis_question, "Hello? World.", "dis");
neovim_test!(textobjects, yis_yank, "Hello. World.", "yisP");
neovim_test!(textobjects, cis_change, "Hello. World.", "cisnew<Esc>");

// Around sentence (as)
neovim_test!(textobjects, das_basic, "Hello. World.", "das");
neovim_test!(textobjects, das_includes_trailing, "Hello.  World.  End.", "das");
neovim_test!(textobjects, yas_yank, "Hello. World.", "yasP");

// Cursor in middle
neovim_test!(textobjects, dis_middle, "First. Second. Third.", cursor(0, 10), "dis");
neovim_test!(textobjects, das_middle, "First. Second. Third.", cursor(0, 10), "das");

// NEW: Sentence edge cases
neovim_test!(textobjects, dis_single, "Hello.", "dis");
neovim_test!(textobjects, das_single, "Hello.", "das");
neovim_test!(textobjects, dis_no_period, "Hello world", "dis");
neovim_test!(textobjects, das_no_period, "Hello world", "das");
neovim_test!(textobjects, dis_multiline, "Hello world.\nNext sentence.", "dis");
neovim_test!(textobjects, das_multiline, "Hello world.\nNext sentence.", "das");
neovim_test!(textobjects, vis_visual, "Hello. World.", "visd");
neovim_test!(textobjects, vas_visual, "Hello. World.", "vasd");
neovim_test!(textobjects, dis_unicode, "日本語。次の文。", "dis");
neovim_test!(textobjects, dis_abbreviation, "Dr. Smith is here.", cursor(0, 5), "dis");
neovim_test!(textobjects, dis_parens_end, "Hello (world). Next.", "dis");
neovim_test!(textobjects, das_parens_end, "Hello (world). Next.", "das");
neovim_test!(textobjects, cis_change_multi, "First. Second. Third.", cursor(0, 10), "cisnew<Esc>");
neovim_test!(textobjects, cas_change, "First. Second. Third.", cursor(0, 10), "casnew<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// TAG OBJECTS (it, at)
// ═══════════════════════════════════════════════════════════════════════════════

// Inner tag (it)
neovim_test!(textobjects, dit_basic, "<div>hello</div>", cursor(0, 5), "dit");
neovim_test!(textobjects, yit_yank, "<div>hello</div>", cursor(0, 5), "yitP");
neovim_test!(textobjects, cit_change, "<div>hello</div>", cursor(0, 5), "citnew<Esc>");
neovim_test!(textobjects, vit_select, "<div>hello</div>", cursor(0, 5), "vitd");

// Around tag (at)
neovim_test!(textobjects, dat_basic, "<div>hello</div>", cursor(0, 5), "dat");
neovim_test!(textobjects, yat_yank, "<div>hello</div>", cursor(0, 5), "yatP");

// Nested tags
neovim_test!(textobjects, dit_nested, "<outer><inner>text</inner></outer>", cursor(0, 14), "dit");
neovim_test!(textobjects, dat_nested, "<outer><inner>text</inner></outer>", cursor(0, 14), "dat");

// Tags with attributes
neovim_test!(textobjects, dit_with_attr, "<div class=\"foo\">bar</div>", cursor(0, 17), "dit");
neovim_test!(textobjects, dat_with_attr, "<div class=\"foo\">bar</div>", cursor(0, 17), "dat");

// NEW: Tag edge cases
neovim_test!(textobjects, dit_empty, "<div></div>", cursor(0, 5), "dit");
neovim_test!(textobjects, dat_empty, "<div></div>", cursor(0, 5), "dat");
neovim_test!(textobjects, dit_self_closing, "<br/>", cursor(0, 2), "dit");
neovim_test!(textobjects, dit_multiline, "<div>\n  content\n</div>", cursor(1, 2), "dit");
neovim_test!(textobjects, dat_multiline, "<div>\n  content\n</div>", cursor(1, 2), "dat");
neovim_test!(textobjects, dit_deeply_nested, "<a><b><c>text</c></b></a>", cursor(0, 10), "dit");
neovim_test!(textobjects, dat_deeply_nested, "<a><b><c>text</c></b></a>", cursor(0, 10), "dat");
neovim_test!(textobjects, dit_unicode, "<div>日本語</div>", cursor(0, 5), "dit");
neovim_test!(textobjects, vit_visual, "<div>hello</div>", cursor(0, 5), "vitd");
neovim_test!(textobjects, vat_visual, "<div>hello</div>", cursor(0, 5), "vatd");
neovim_test!(textobjects, cit_change_nested, "<outer><inner>text</inner></outer>", cursor(0, 7), "citnew<Esc>");
neovim_test!(textobjects, cat_change, "<div>hello</div>", cursor(0, 5), "catnew<Esc>");
neovim_test!(textobjects, dit_multiple_attrs, "<div id=\"x\" class=\"y\">text</div>", cursor(0, 22), "dit");

// ═══════════════════════════════════════════════════════════════════════════════
// COMBINED OPERATORS + TEXT OBJECTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(textobjects, d_then_p_iw, "hello world", "diwP");
neovim_test!(textobjects, y_then_paste_i_paren, "foo (bar) baz", cursor(0, 5), "yi($p");
neovim_test!(textobjects, double_textobj, "(hello) (world)", "di(llllldi(");

// NEW: More combined tests
neovim_test!(textobjects, d_then_p_aw, "hello world", "dawP");
neovim_test!(textobjects, c_iw_then_type, "hello world", cursor(0, 6), "ciwfoo<Esc>");
neovim_test!(textobjects, y_i_brace_paste, "{ code }", cursor(0, 2), "yi{$p");
neovim_test!(textobjects, d_a_dquote_multi, "\"a\" \"b\"", "da\"llda\"");
neovim_test!(textobjects, yy_diw_p, "hello world", "yydiwP");
neovim_test!(textobjects, di_paren_di_brace, "({hello})", cursor(0, 2), "di(jdi{");
neovim_test!(textobjects, visual_iw_extend, "one two three", "viwiwld");
neovim_test!(textobjects, visual_aw_extend, "one two three", "vawawld");
neovim_test!(textobjects, ci_dquote_escape, "\"hello\" world", "ci\"bye<Esc>");
neovim_test!(textobjects, gu_iw_lowercase, "HELLO world", "guiw");
neovim_test!(textobjects, gU_iw_uppercase, "hello WORLD", "gUiw");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES - COMPREHENSIVE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(textobjects, diw_at_eof_no_newline, "hello world", cursor(0, 10), "diw");
neovim_test!(textobjects, di_paren_empty_edge, "()", cursor(0, 1), "di(");
neovim_test!(textobjects, di_quote_empty_edge, "\"\"", cursor(0, 1), "di\"");
neovim_test!(textobjects, da_paren_unmatched, "(hello", cursor(0, 1), "da(");
neovim_test!(textobjects, di_paren_multiline, "(\nhello\nworld\n)", cursor(1, 0), "di(");

// NEW: More edge cases
neovim_test!(textobjects, di_brace_unmatched, "{hello", cursor(0, 1), "di{");
neovim_test!(textobjects, di_bracket_unmatched, "[hello", cursor(0, 1), "di[");
neovim_test!(textobjects, di_angle_unmatched, "<hello", cursor(0, 1), "di<");
neovim_test!(textobjects, diw_single_char_word, "a b c d", cursor(0, 2), "diw");
neovim_test!(textobjects, daw_single_char_word, "a b c d", cursor(0, 2), "daw");
neovim_test!(textobjects, di_dquote_unmatched, "\"hello", cursor(0, 1), "di\"");
neovim_test!(textobjects, di_squote_unmatched, "'hello", cursor(0, 1), "di'");
neovim_test!(textobjects, di_backtick_unmatched, "`hello", cursor(0, 1), "di`");
neovim_test!(textobjects, diw_at_empty_line, "hello\n\nworld", cursor(1, 0), "diw");
neovim_test!(textobjects, daw_at_empty_line, "hello\n\nworld", cursor(1, 0), "daw");
neovim_test!(textobjects, dis_empty_buffer, "", "dis");
neovim_test!(textobjects, das_empty_buffer, "", "das");
neovim_test!(textobjects, dit_no_tag, "hello world", "dit");
neovim_test!(textobjects, dat_no_tag, "hello world", "dat");
neovim_test!(textobjects, di_paren_only_whitespace, "(   )", cursor(0, 2), "di(");
neovim_test!(textobjects, di_brace_only_whitespace, "{   }", cursor(0, 2), "di{");
neovim_test!(textobjects, di_dquote_only_whitespace, "\"   \"", cursor(0, 2), "di\"");

// Pipeline hardening regressions: visual operator selection path
neovim_test!(textobjects, visual_gu_textobject_selection, "HELLO world", "viwgu");
neovim_test!(textobjects, visual_gU_textobject_selection, "hello world", "viwgU");
neovim_test!(textobjects, visual_gtilde_textobject_selection, "Hello world", "viwg~");
