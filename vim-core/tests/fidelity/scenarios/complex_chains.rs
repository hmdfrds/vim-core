// Scenario fidelity tests: Complex Chains
//
// The hardest multi-step workflows combining motions, operators, text objects, and insert.

// ═══════════════════════════════════════════════════════════════════════════════
// MOTION + OPERATOR + INSERT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, search_change_append, "hello world target", "/target<CR>ciwRPLCD<Esc>A!<Esc>");
neovim_test!(scenarios, find_delete_insert, "prefix_name()", "df_iget_<Esc>");
neovim_test!(scenarios, goto_line_change_return, "l1\nl2\nl3", "2GccNEW<Esc>gg");
neovim_test!(scenarios, word_delete_word_paste, "alpha beta gamma", "dwwP");
neovim_test!(scenarios, visual_yank_search_paste, "src dst src", "viwy/dst<CR>viwp");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT CHAINS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cw_dot_dot, "old old old", "cwnew<Esc>w.w.");
neovim_test!(scenarios, x_dot_chain, "abcdefgh", "x.x.x.");
neovim_test!(scenarios, dd_dot_chain, "l1\nl2\nl3\nl4", "dd...");
neovim_test!(scenarios, A_semicolon_dot, "line1\nline2\nline3", "A;<Esc>j.j.");
neovim_test!(scenarios, I_comment_dot, "code1\ncode2\ncode3", "I// <Esc>j.j.");
neovim_test!(scenarios, ciw_dot_repeat, "aa bb aa bb", "ciwXX<Esc>ww.");

// ═══════════════════════════════════════════════════════════════════════════════
// MACRO + OPERATOR COMBOS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, macro_cw_replay, "old old old", "qacwnew<Esc>wq2@a");
neovim_test!(scenarios, macro_dd_replay, "l1\nl2\nl3\nl4", "qaddq3@a");
neovim_test!(scenarios, macro_indent_replay, "a\nb\nc\nd", "qa>>jq3@a");
neovim_test!(scenarios, macro_append_lines, "a\nb\nc", "qaA!<Esc>jq2@a");

// ═══════════════════════════════════════════════════════════════════════════════
// TEXT OBJECT CHAINS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, diw_then_paste, "swap these words", cursor(0, 5), "diwwP");
neovim_test!(scenarios, ci_quote_search_repeat, "\"a\" and \"b\"", cursor(0, 1), "ci\"X<Esc>/\"<CR>.");
neovim_test!(scenarios, change_parens_twice, "(old1) and (old2)", cursor(0, 1), "ci(new1<Esc>f(ci(new2<Esc>");
neovim_test!(scenarios, delete_inner_paste_after, "remove (keep) rest", cursor(0, 8), "yi(da($p");

// ═══════════════════════════════════════════════════════════════════════════════
// REGISTER CHAINS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, named_reg_swap, "alpha beta", "\"adiw$\"bdiw0\"bp$\"ap");
neovim_test!(scenarios, yank_delete_paste, "copy delete target", "yiwwdaw$p");
neovim_test!(scenarios, multi_register_workflow, "aaa bbb ccc", "\"ayiww\"byiww\"cyiw$\"ap\"bp\"cp");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL + OPERATOR CHAINS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, visual_select_uppercase, "hello world", "viwgUwviwgU");
neovim_test!(scenarios, visual_line_indent_twice, "code\ncode", "Vj>>>");
neovim_test!(scenarios, visual_delete_insert, "old content here", "viwdiFresh<Esc>");
neovim_test!(scenarios, visual_yank_search_put, "src and dst", "viwy/dst<CR>p");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO + REDO IN CHAINS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, change_undo_redo, "original", "cwchanged<Esc>u<C-r>");
neovim_test!(scenarios, multi_edit_undo_partial, "aaa bbb ccc", "ciwXXX<Esc>wciwYYY<Esc>u");
neovim_test!(scenarios, dd_undo_dd_paste, "l1\nl2\nl3", "dduddjp");
neovim_test!(scenarios, macro_then_undo, "abc\ndef", "qaA!<Esc>jq@auu");

// ═══════════════════════════════════════════════════════════════════════════════
// MIXED MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, hjkl_sequence, "abc\ndef\nghi", "lljjhk");
neovim_test!(scenarios, word_back_word, "one two three four", "wwwbb");
neovim_test!(scenarios, gg_G_gg, "l1\nl2\nl3\nl4\nl5", "GggG");
neovim_test!(scenarios, dollar_zero_caret, "  hello world  ", "$0^");
neovim_test!(scenarios, find_semicolon_comma, "aXbXcXd", "fX;,");
neovim_test!(scenarios, percent_chain, "(hello (world))", "%ll%");

// ═══════════════════════════════════════════════════════════════════════════════
// MARK + OPERATOR CHAINS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, mark_yank_paste, "aaa bbb\nccc", "majjy'aGp");
neovim_test!(scenarios, mark_delete_range, "l1\nl2\nl3\nl4", "jma2jd'a");
neovim_test!(scenarios, mark_visual_gU, "lower\ncase\nupper", "majjv'agU");
neovim_test!(scenarios, mark_jump_cw, "start\nmiddle\ntarget", "2jmawgg/target<CR>cwfound<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH + REGISTER CHAINS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, search_yank_to_reg, "start target end", "/target<CR>\"ayiw$\"ap");
neovim_test!(scenarios, star_delete_paste, "foo bar foo baz", "*Ndaw$p");
neovim_test!(scenarios, search_ci_quote, "fn(\"old\")", "/old<CR>ci\"new<Esc>");
neovim_test!(scenarios, search_visual_gU, "hello target world", "/target<CR>viwgU");

// ═══════════════════════════════════════════════════════════════════════════════
// LONG MULTI-STEP SEQUENCES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, full_refactor, "let old = fn(x);", "wciwresult<Esc>ww ciwcall<Esc>");
neovim_test!(scenarios, comment_copy_edit, "original()", "yypI// <Esc>jcwmodified<Esc>");
neovim_test!(scenarios, extract_and_call, "  long_expr + calc", "^yw o  let val = <C-r>\";<Esc>");
neovim_test!(scenarios, swap_two_words, "beta alpha", "dwwP");
neovim_test!(scenarios, duplicate_modify_3, "template", "yy2p2GcwA<Esc>3GcwB<Esc>");
neovim_test!(scenarios, wrap_fn_ci, "(old_val)", cursor(0, 1), "ci(wrapper(old_val)<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// EXTREME CHAINS (7+ steps)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, refactor_full, "fn old() {\n    dbg!(x);\n    old();\n}", "wcwnew<Esc>jddjcwnew<Esc>");
neovim_test!(scenarios, build_function, "", "ifn process(data: &str) -> Result<()> {<CR>    todo!()<CR>}<Esc>");
neovim_test!(scenarios, multi_file_edit_sim, "import old\n\nold.call()\nold.other()", "ciwmod<Esc>2jwciw mod<Esc>jwciwmod<Esc>");
neovim_test!(scenarios, reorder_and_edit, "c\nb\na", "GddggPjddGpggcwX<Esc>");
neovim_test!(scenarios, macro_complex, "fn a() {}\nfn b() {}\nfn c() {}", "qaIpub <Esc>jq2@a");

