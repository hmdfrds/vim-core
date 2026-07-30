// Text Object Edge Cases fidelity tests.
//
// Comprehensive edge cases for text objects that go beyond basic coverage:
// - Counted text objects (2iw, 3a(, etc.)
// - Nested text objects
// - Boundary and empty cases
// - Cross-line text objects
// - Mixed and unusual content

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTED TEXT OBJECTS — WORD
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, d_2iw, "one two three four", "d2iw");
neovim_test!(scenarios, d_3iw, "one two three four five", "d3iw");
neovim_test!(scenarios, d_2aw, "one two three four", "d2aw");
neovim_test!(scenarios, d_3aw, "one two three four five", "d3aw");
neovim_test!(scenarios, c_2iw, "one two three four", "c2iwX<Esc>");
neovim_test!(scenarios, c_3aw, "one two three four", "c3awX<Esc>");
neovim_test!(scenarios, y_2iw, "one two three four", "y2iw$p");
neovim_test!(scenarios, y_2aw, "one two three four", "y2aw$p");
neovim_test!(scenarios, d_2iW, "foo.bar baz.qux end", "d2iW");
neovim_test!(scenarios, d_2aW, "foo.bar baz.qux end", "d2aW");
neovim_test!(scenarios, v_2iw, "one two three four", "v2iwd");
neovim_test!(scenarios, v_3aw, "one two three four five", "v3awd");

// Counted word from middle of word
neovim_test!(scenarios, d_2iw_from_mid, "one two three four", cursor(0, 5), "d2iw");
neovim_test!(scenarios, d_2aw_from_mid, "one two three four", cursor(0, 5), "d2aw");

// Count exceeds available words
neovim_test!(scenarios, d_99iw, "one two three", "d99iw");
neovim_test!(scenarios, d_99aw, "one two three", "d99aw");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTED TEXT OBJECTS — BRACKETS
// ═══════════════════════════════════════════════════════════════════════════════

// 2i( means outer paren when inside nested parens
neovim_test!(scenarios, d_2i_paren, "((inner))", cursor(0, 3), "d2i(");
neovim_test!(scenarios, d_2a_paren, "((inner))", cursor(0, 3), "d2a(");
neovim_test!(scenarios, c_2i_paren, "((inner))", cursor(0, 3), "c2i(X<Esc>");
neovim_test!(scenarios, y_2i_paren, "((inner))", cursor(0, 3), "y2i($p");

// Triple-nested
neovim_test!(scenarios, d_3i_paren, "(((deep)))", cursor(0, 4), "d3i(");
neovim_test!(scenarios, d_3a_paren, "(((deep)))", cursor(0, 4), "d3a(");

// Same for braces
neovim_test!(scenarios, d_2i_brace, "{{inner}}", cursor(0, 3), "d2i{");
neovim_test!(scenarios, d_2a_brace, "{{inner}}", cursor(0, 3), "d2a{");
neovim_test!(scenarios, d_3i_brace, "{{{deep}}}", cursor(0, 4), "d3i{");

// Same for brackets
neovim_test!(scenarios, d_2i_bracket, "[[inner]]", cursor(0, 3), "d2i[");
neovim_test!(scenarios, d_2a_bracket, "[[inner]]", cursor(0, 3), "d2a[");

// Same for angle brackets
neovim_test!(scenarios, d_2i_angle, "<<inner>>", cursor(0, 3), "d2i<lt>");
neovim_test!(scenarios, d_2a_angle, "<<inner>>", cursor(0, 3), "d2a<lt>");

// Counted visual selection
neovim_test!(scenarios, v_2i_paren, "((inner))", cursor(0, 3), "v2i(d");
neovim_test!(scenarios, v_2a_paren, "((inner))", cursor(0, 3), "v2a(d");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTED TEXT OBJECTS — QUOTES
// ═══════════════════════════════════════════════════════════════════════════════

// Count has no standard meaning for quotes (not nested), but test it anyway
neovim_test!(scenarios, d_2i_dquote, "\"hello\"", cursor(0, 3), "d2i\"");
neovim_test!(scenarios, d_2a_dquote, "\"hello\"", cursor(0, 3), "d2a\"");
neovim_test!(scenarios, d_2i_squote, "'hello'", cursor(0, 3), "d2i'");
neovim_test!(scenarios, d_2a_squote, "'hello'", cursor(0, 3), "d2a'");
neovim_test!(scenarios, d_2i_backtick, "`hello`", cursor(0, 3), "d2i`");

// ═══════════════════════════════════════════════════════════════════════════════
// COUNTED TEXT OBJECTS — SENTENCE/PARAGRAPH
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, d_2is, "First. Second. Third.", "d2is");
neovim_test!(scenarios, d_2as, "First. Second. Third.", "d2as");
neovim_test!(scenarios, d_3is, "First. Second. Third. Fourth.", "d3is");
neovim_test!(scenarios, d_2ip, "para1\n\npara2\n\npara3", "d2ip");
neovim_test!(scenarios, d_2ap, "para1\n\npara2\n\npara3", "d2ap");

// ═══════════════════════════════════════════════════════════════════════════════
// NESTED TEXT OBJECTS — MIXED BRACKET TYPES
// ═══════════════════════════════════════════════════════════════════════════════

// Inner braces inside parens
neovim_test!(scenarios, di_brace_in_paren, "({content})", cursor(0, 3), "di{");
neovim_test!(scenarios, di_paren_around_brace, "({content})", cursor(0, 3), "di(");
neovim_test!(scenarios, da_brace_in_paren, "({content})", cursor(0, 3), "da{");
neovim_test!(scenarios, da_paren_around_brace, "({content})", cursor(0, 3), "da(");

// Inner parens inside braces
neovim_test!(scenarios, di_paren_in_brace, "{(content)}", cursor(0, 3), "di(");
neovim_test!(scenarios, di_brace_around_paren, "{(content)}", cursor(0, 3), "di{");

// Brackets inside parens inside braces
neovim_test!(scenarios, di_bracket_in_nested, "{([content])}", cursor(0, 4), "di[");
neovim_test!(scenarios, di_paren_in_nested, "{([content])}", cursor(0, 4), "di(");
neovim_test!(scenarios, di_brace_in_nested, "{([content])}", cursor(0, 4), "di{");

// Quotes inside brackets
neovim_test!(scenarios, di_dquote_in_paren, "(\"content\")", cursor(0, 4), "di\"");
neovim_test!(scenarios, di_paren_around_dquote, "(\"content\")", cursor(0, 4), "di(");
neovim_test!(scenarios, di_squote_in_brace, "{'content'}", cursor(0, 4), "di'");
neovim_test!(scenarios, di_brace_around_squote, "{'content'}", cursor(0, 4), "di{");

// ═══════════════════════════════════════════════════════════════════════════════
// CROSS-LINE TEXT OBJECTS
// ═══════════════════════════════════════════════════════════════════════════════

// Multi-line parens
neovim_test!(scenarios, edge_di_paren_multiline, "(\n  hello\n  world\n)", cursor(1, 2), "di(");
neovim_test!(scenarios, da_paren_multiline, "(\n  hello\n  world\n)", cursor(1, 2), "da(");
neovim_test!(scenarios, ci_paren_multiline, "(\n  hello\n  world\n)", cursor(1, 2), "ci(X<Esc>");
neovim_test!(scenarios, yi_paren_multiline, "(\n  hello\n  world\n)", cursor(1, 2), "yi(Gp");

// Multi-line braces (function body)
neovim_test!(scenarios, di_brace_multiline_fn, "fn() {\n  let x = 1;\n  let y = 2;\n}", cursor(1, 0), "di{");
neovim_test!(scenarios, da_brace_multiline_fn, "fn() {\n  let x = 1;\n  let y = 2;\n}", cursor(1, 0), "da{");
neovim_test!(scenarios, ci_brace_multiline_fn, "fn() {\n  let x = 1;\n  let y = 2;\n}", cursor(1, 0), "ci{X<Esc>");

// Multi-line brackets (array)
neovim_test!(scenarios, edge_di_bracket_multiline, "[\n  item1,\n  item2,\n]", cursor(1, 2), "di[");
neovim_test!(scenarios, da_bracket_multiline, "[\n  item1,\n  item2,\n]", cursor(1, 2), "da[");

// Multi-line tags
neovim_test!(scenarios, dit_multiline_full, "<div>\n  <p>hello</p>\n  <p>world</p>\n</div>", cursor(1, 2), "dit");
neovim_test!(scenarios, dat_multiline_full, "<div>\n  <p>hello</p>\n  <p>world</p>\n</div>", cursor(1, 2), "dat");

// Sentence spanning lines
neovim_test!(scenarios, dis_cross_line, "Hello\nworld. Next.", "dis");
neovim_test!(scenarios, das_cross_line, "Hello\nworld. Next.", "das");

// ═══════════════════════════════════════════════════════════════════════════════
// EMPTY AND WHITESPACE-ONLY TEXT OBJECTS
// ═══════════════════════════════════════════════════════════════════════════════

// Empty brackets
neovim_test!(scenarios, edge_di_paren_empty, "()", cursor(0, 0), "di(");
neovim_test!(scenarios, edge_da_paren_empty, "()", cursor(0, 0), "da(");
neovim_test!(scenarios, ci_paren_empty, "()", cursor(0, 0), "ci(X<Esc>");
neovim_test!(scenarios, edge_di_brace_empty, "{}", cursor(0, 0), "di{");
neovim_test!(scenarios, edge_da_brace_empty, "{}", cursor(0, 0), "da{");
neovim_test!(scenarios, edge_di_bracket_empty, "[]", cursor(0, 0), "di[");
neovim_test!(scenarios, edge_da_bracket_empty, "[]", cursor(0, 0), "da[");

// Whitespace-only content
neovim_test!(scenarios, di_paren_spaces, "(   )", cursor(0, 2), "di(");
neovim_test!(scenarios, di_brace_spaces, "{   }", cursor(0, 2), "di{");
neovim_test!(scenarios, di_bracket_spaces, "[   ]", cursor(0, 2), "di[");
neovim_test!(scenarios, edge_di_dquote_spaces, "\"   \"", cursor(0, 2), "di\"");

// Empty quotes
neovim_test!(scenarios, di_dquote_empty_tobj, "\"\"", cursor(0, 0), "di\"");
neovim_test!(scenarios, da_dquote_empty_tobj, "\"\"", cursor(0, 0), "da\"");
neovim_test!(scenarios, ci_dquote_empty, "\"\"", cursor(0, 0), "ci\"X<Esc>");
neovim_test!(scenarios, di_squote_empty_tobj, "''", cursor(0, 0), "di'");
neovim_test!(scenarios, da_squote_empty_tobj, "''", cursor(0, 0), "da'");
neovim_test!(scenarios, di_backtick_empty_tobj, "``", cursor(0, 0), "di`");

// Empty tags
neovim_test!(scenarios, dit_empty_tobj, "<div></div>", cursor(0, 5), "dit");
neovim_test!(scenarios, dat_empty_tobj, "<div></div>", cursor(0, 5), "dat");
neovim_test!(scenarios, cit_empty, "<div></div>", cursor(0, 5), "citX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// UNMATCHED AND EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

// Unmatched brackets (should be no-op or partial)
neovim_test!(scenarios, di_paren_unmatched_open, "(hello", cursor(0, 2), "di(");
neovim_test!(scenarios, di_paren_unmatched_close, "hello)", cursor(0, 2), "di(");
neovim_test!(scenarios, di_brace_unmatched_open, "{hello", cursor(0, 2), "di{");
neovim_test!(scenarios, di_brace_unmatched_close, "hello}", cursor(0, 2), "di{");
neovim_test!(scenarios, di_bracket_unmatched_open, "[hello", cursor(0, 2), "di[");
neovim_test!(scenarios, di_bracket_unmatched_close, "hello]", cursor(0, 2), "di[");

// No enclosing bracket at all
neovim_test!(scenarios, di_paren_none, "hello world", cursor(0, 5), "di(");
neovim_test!(scenarios, di_brace_none, "hello world", cursor(0, 5), "di{");
neovim_test!(scenarios, di_bracket_none, "hello world", cursor(0, 5), "di[");
neovim_test!(scenarios, di_dquote_none, "hello world", cursor(0, 5), "di\"");
neovim_test!(scenarios, di_squote_none, "hello world", cursor(0, 5), "di'");

// Cursor on the bracket itself
neovim_test!(scenarios, di_paren_on_open, "(hello)", cursor(0, 0), "di(");
neovim_test!(scenarios, di_paren_on_close, "(hello)", cursor(0, 6), "di(");
neovim_test!(scenarios, da_paren_on_open, "(hello)", cursor(0, 0), "da(");
neovim_test!(scenarios, da_paren_on_close, "(hello)", cursor(0, 6), "da(");
neovim_test!(scenarios, di_brace_on_open, "{hello}", cursor(0, 0), "di{");
neovim_test!(scenarios, di_brace_on_close, "{hello}", cursor(0, 6), "di{");

// Cursor on a quote character
neovim_test!(scenarios, di_dquote_on_open, "\"hello\"", cursor(0, 0), "di\"");
neovim_test!(scenarios, di_dquote_on_close, "\"hello\"", cursor(0, 6), "di\"");
neovim_test!(scenarios, da_dquote_on_open, "\"hello\"", cursor(0, 0), "da\"");
neovim_test!(scenarios, di_squote_on_open, "'hello'", cursor(0, 0), "di'");
neovim_test!(scenarios, di_squote_on_close, "'hello'", cursor(0, 6), "di'");

// ═══════════════════════════════════════════════════════════════════════════════
// ADJACENT BRACKETS
// ═══════════════════════════════════════════════════════════════════════════════

// Multiple adjacent brackets — cursor position determines which pair
neovim_test!(scenarios, di_paren_first_of_many, "(a)(b)(c)", cursor(0, 1), "di(");
neovim_test!(scenarios, di_paren_second_of_many, "(a)(b)(c)", cursor(0, 4), "di(");
neovim_test!(scenarios, di_paren_third_of_many, "(a)(b)(c)", cursor(0, 7), "di(");
neovim_test!(scenarios, da_paren_first_of_many, "(a)(b)(c)", cursor(0, 1), "da(");
neovim_test!(scenarios, da_paren_second_of_many, "(a)(b)(c)", cursor(0, 4), "da(");
neovim_test!(scenarios, da_paren_third_of_many, "(a)(b)(c)", cursor(0, 7), "da(");

// Between brackets (cursor between two pairs)
neovim_test!(scenarios, di_paren_between, "(a) (b)", cursor(0, 3), "di(");

// Adjacent quotes
neovim_test!(scenarios, di_dquote_first_adj, "\"a\"\"b\"", cursor(0, 1), "di\"");
neovim_test!(scenarios, di_dquote_second_adj, "\"a\"\"b\"", cursor(0, 4), "di\"");

// ═══════════════════════════════════════════════════════════════════════════════
// CURSOR BEFORE TEXT OBJECT (SEEKING FORWARD)
// ═══════════════════════════════════════════════════════════════════════════════

// For quotes and brackets, cursor before the object should seek forward
neovim_test!(scenarios, di_dquote_before_seek, "hello \"world\"", cursor(0, 0), "di\"");
neovim_test!(scenarios, ci_dquote_before_seek, "hello \"world\"", cursor(0, 0), "ci\"X<Esc>");
neovim_test!(scenarios, di_paren_before_seek, "hello (world)", cursor(0, 0), "di(");
neovim_test!(scenarios, ci_paren_before_seek, "hello (world)", cursor(0, 0), "ci(X<Esc>");
neovim_test!(scenarios, di_brace_before_seek, "hello {world}", cursor(0, 0), "di{");
neovim_test!(scenarios, di_bracket_before_seek, "hello [world]", cursor(0, 0), "di[");
neovim_test!(scenarios, di_squote_before_seek, "hello 'world'", cursor(0, 0), "di'");
neovim_test!(scenarios, di_backtick_before_seek, "hello `world`", cursor(0, 0), "di`");

// ═══════════════════════════════════════════════════════════════════════════════
// SINGLE CHARACTER INSIDE TEXT OBJECTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, di_paren_single_char, "(x)", cursor(0, 1), "di(");
neovim_test!(scenarios, da_paren_single_char, "(x)", cursor(0, 1), "da(");
neovim_test!(scenarios, di_brace_single_char, "{x}", cursor(0, 1), "di{");
neovim_test!(scenarios, di_bracket_single_char, "[x]", cursor(0, 1), "di[");
neovim_test!(scenarios, di_dquote_single_char, "\"x\"", cursor(0, 1), "di\"");
neovim_test!(scenarios, di_squote_single_char, "'x'", cursor(0, 1), "di'");
neovim_test!(scenarios, di_backtick_single_char, "`x`", cursor(0, 1), "di`");

// ═══════════════════════════════════════════════════════════════════════════════
// TEXT OBJECTS WITH UNICODE CONTENT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, diw_cjk_chars, "hello日本語world", cursor(0, 5), "diw");
neovim_test!(scenarios, daw_cjk_chars, "hello 日本語 world", cursor(0, 6), "daw");
neovim_test!(scenarios, di_paren_cjk, "(日本語)", cursor(0, 1), "di(");
neovim_test!(scenarios, da_paren_cjk, "(日本語)", cursor(0, 1), "da(");
neovim_test!(scenarios, di_dquote_cjk, "\"日本語\"", cursor(0, 1), "di\"");
neovim_test!(scenarios, di_paren_emoji, "(👍🎉)", cursor(0, 1), "di(");
neovim_test!(scenarios, di_dquote_emoji, "\"👍🎉\"", cursor(0, 1), "di\"");
neovim_test!(scenarios, dit_cjk, "<div>日本語</div>", cursor(0, 5), "dit");
neovim_test!(scenarios, dat_cjk, "<div>日本語</div>", cursor(0, 5), "dat");

// ═══════════════════════════════════════════════════════════════════════════════
// TEXT OBJECTS WITH SPECIAL CONTENT
// ═══════════════════════════════════════════════════════════════════════════════

// Content with newlines inside brackets
neovim_test!(scenarios, di_paren_newline_content, "(hello\nworld)", cursor(0, 3), "di(");
neovim_test!(scenarios, di_brace_newline_content, "{hello\nworld}", cursor(0, 3), "di{");
neovim_test!(scenarios, di_bracket_newline_content, "[hello\nworld]", cursor(0, 3), "di[");

// Content with tabs
neovim_test!(scenarios, di_paren_tab_content, "(hello\tworld)", cursor(0, 3), "di(");
neovim_test!(scenarios, di_dquote_tab_content, "\"hello\tworld\"", cursor(0, 3), "di\"");

// Content with nested brackets of same type
neovim_test!(scenarios, di_paren_same_nested, "(a(b(c))d)", cursor(0, 6), "di(");
neovim_test!(scenarios, da_paren_same_nested, "(a(b(c))d)", cursor(0, 6), "da(");
neovim_test!(scenarios, di_brace_same_nested, "{a{b{c}}d}", cursor(0, 6), "di{");
neovim_test!(scenarios, di_bracket_same_nested, "[a[b[c]]d]", cursor(0, 6), "di[");

// ═══════════════════════════════════════════════════════════════════════════════
// TEXT OBJECTS — TAG EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

// Tags with attributes
neovim_test!(scenarios, dit_with_class, "<div class=\"container\">content</div>", cursor(0, 25), "dit");
neovim_test!(scenarios, dat_with_class, "<div class=\"container\">content</div>", cursor(0, 25), "dat");
neovim_test!(scenarios, dit_with_multiple_attrs, "<input type=\"text\" name=\"foo\" />", cursor(0, 10), "dit");

// Nested tags of same type
neovim_test!(scenarios, dit_same_nested, "<div><div>inner</div></div>", cursor(0, 11), "dit");
neovim_test!(scenarios, dat_same_nested, "<div><div>inner</div></div>", cursor(0, 11), "dat");

// Tags with no content
neovim_test!(scenarios, dit_self_closing_tobj, "<br/>", cursor(0, 2), "dit");
neovim_test!(scenarios, dit_void_element, "<img src=\"x\">", cursor(0, 5), "dit");

// Tags spanning multiple lines
neovim_test!(scenarios, dit_multiline_content, "<ul>\n  <li>a</li>\n  <li>b</li>\n</ul>", cursor(1, 4), "dit");
neovim_test!(scenarios, dat_multiline_content, "<ul>\n  <li>a</li>\n  <li>b</li>\n</ul>", cursor(1, 4), "dat");

// ═══════════════════════════════════════════════════════════════════════════════
// TEXT OBJECTS CHAINED WITH DOT REPEAT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, dot_diw_chain, "one two three four", "diww.");
neovim_test!(scenarios, dot_daw_chain, "one two three four", "daw.");
neovim_test!(scenarios, dot_ciw_chain, "one two three four", "ciwX<Esc>w.");
neovim_test!(scenarios, dot_di_paren_chain, "(a) (b) (c)", cursor(0, 1), "di(f(.");
neovim_test!(scenarios, dot_ci_dquote_chain, "\"a\" \"b\" \"c\"", cursor(0, 1), "ci\"X<Esc>f\".");
neovim_test!(scenarios, dot_da_paren_chain, "(a) (b) (c)", cursor(0, 1), "da(.");
neovim_test!(scenarios, dot_da_dquote_chain, "\"a\" \"b\" \"c\"", cursor(0, 1), "da\".");

// ═══════════════════════════════════════════════════════════════════════════════
// SENTENCE TEXT OBJECTS — EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

// Multiple spaces after period
neovim_test!(scenarios, dis_double_space, "Hello.  World.", "dis");
neovim_test!(scenarios, das_double_space, "Hello.  World.", "das");

// Sentence with parens at end
neovim_test!(scenarios, dis_paren_end, "Hello world (test). Next.", "dis");
neovim_test!(scenarios, das_paren_end, "Hello world (test). Next.", "das");

// Single sentence (entire buffer is one sentence)
neovim_test!(scenarios, dis_whole_buffer, "Hello world.", "dis");
neovim_test!(scenarios, das_whole_buffer, "Hello world.", "das");

// Multiple sentences on same line
neovim_test!(scenarios, dis_multi_same_line, "A. B. C.", cursor(0, 3), "dis");
neovim_test!(scenarios, das_multi_same_line, "A. B. C.", cursor(0, 3), "das");

// ═══════════════════════════════════════════════════════════════════════════════
// PARAGRAPH TEXT OBJECTS — EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

// Single line paragraph
neovim_test!(scenarios, dip_single_line_para, "hello", "dip");
neovim_test!(scenarios, dap_single_line_para, "hello", "dap");

// All empty lines
neovim_test!(scenarios, dip_all_empty, "\n\n\n", cursor(1, 0), "dip");
neovim_test!(scenarios, dap_all_empty, "\n\n\n", cursor(1, 0), "dap");

// Paragraph with trailing blank lines
neovim_test!(scenarios, dip_trailing_blank, "para1\n\n\n", "dip");
neovim_test!(scenarios, dap_trailing_blank, "para1\n\n\n", "dap");

// Three paragraphs
neovim_test!(scenarios, dip_three_paras, "p1\n\np2\n\np3", cursor(2, 0), "dip");
neovim_test!(scenarios, dap_three_paras, "p1\n\np2\n\np3", cursor(2, 0), "dap");

// Paragraph with whitespace-only lines (not blank — they count as content)
neovim_test!(scenarios, dip_whitespace_line, "hello\n   \nworld", "dip");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL MODE TEXT OBJECT EXTENSION
// ═══════════════════════════════════════════════════════════════════════════════

// Repeated text object in visual mode grows selection
neovim_test!(scenarios, viw_iw_extend, "one two three four", "viwiw");
neovim_test!(scenarios, vaw_aw_extend, "one two three four", "vawaw");
neovim_test!(scenarios, vi_paren_extend, "((inner))", cursor(0, 3), "vi(i(");
neovim_test!(scenarios, va_paren_extend, "((inner))", cursor(0, 3), "va(a(");
neovim_test!(scenarios, vi_brace_extend, "{{inner}}", cursor(0, 3), "vi{i{");
neovim_test!(scenarios, vi_dquote_extend, "\"hello\"", cursor(0, 3), "vi\"a\"");
neovim_test!(scenarios, vip_ip_extend, "p1\n\np2\n\np3", "vipip");
neovim_test!(scenarios, vap_ap_extend, "p1\n\np2\n\np3", "vapap");
