// Scenario fidelity tests: Navigation + Edit
//
// Multi-step workflows combining navigation and editing.

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH THEN EDIT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, search_and_change, "hello world target", "/target<CR>cwreplaced<Esc>");
neovim_test!(scenarios, search_and_delete, "keep remove_me keep", "/remove<CR>daw");
neovim_test!(scenarios, search_and_insert, "fn foo()", "/foo<CR>ea_bar<Esc>");
neovim_test!(scenarios, search_backward_edit, "target first second", cursor(0, 18), "?target<CR>cwfound<Esc>");
neovim_test!(scenarios, star_and_change, "foo bar foo", "*cwbaz<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// FIND THEN OPERATE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, find_delete_to, "hello(world)", "dt(");
neovim_test!(scenarios, find_change_to, "prefix_name", "ct_new<Esc>");
neovim_test!(scenarios, find_delete_through, "key: value", "df:");
neovim_test!(scenarios, find_yank_to, "get(first, second)", "yt,$p");
neovim_test!(scenarios, find_and_replace, "old.method()", "fo;rn");
neovim_test!(scenarios, find_semicolon_delete, "a;b;c;d", "f;x;;x");

// ═══════════════════════════════════════════════════════════════════════════════
// LINE JUMP THEN CHANGE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, goto_line_and_edit, "line1\nline2\nline3", "2Gccnew<Esc>");
neovim_test!(scenarios, goto_line_and_delete, "l1\nl2\nl3\nl4", "3Gdd");
neovim_test!(scenarios, goto_end_and_add, "first\nlast", "Goappended<Esc>");
neovim_test!(scenarios, goto_top_and_insert, "first\nlast", "ggOinserted<Esc>");
neovim_test!(scenarios, goto_line_indent, "l1\nl2\nl3", "2G>>");

// ═══════════════════════════════════════════════════════════════════════════════
// MARK BASED EDITING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, mark_return_edit, "hello world", cursor(0, 6), "ma0ciwbye<Esc>`a");
neovim_test!(scenarios, nav_mark_delete_range, "l1\nl2\nl3\nl4", cursor(0, 0), "majjd'a");
neovim_test!(scenarios, nav_mark_yank_range, "l1\nl2\nl3", cursor(0, 0), "majjy'a0p");
neovim_test!(scenarios, chain_gg_cw, "l1\nl2\nl3\nl4", cursor(3, 0), "ggcwNEW<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// PARAGRAPH JUMP + EDIT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, para_jump_cw, "intro\n\ntarget word\n\nend", "}wcwnew<Esc>");
neovim_test!(scenarios, para_jump_dd, "keep\n\ndelete\n\nkeep2", "}jdd");
neovim_test!(scenarios, para_jump_O, "para1\n\npara2", "}Oinserted<Esc>");
neovim_test!(scenarios, para_back_edit, "para1\n\npara2", cursor(2, 0), "{cwchanged<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH + VISUAL
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, search_visual_delete, "start target end", "v/end<CR>d");
neovim_test!(scenarios, search_visual_yank, "start target end", "v/target<CR>y");
neovim_test!(scenarios, search_visual_change, "start target end", "v/target<CR>cnew<Esc>");
neovim_test!(scenarios, star_visual_select, "foo bar foo", "v*");

// ═══════════════════════════════════════════════════════════════════════════════
// JUMPLIST WORKFLOWS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, jump_gg_G_ctrl_o, "l1\nl2\nl3\nl4\nl5", "G<C-o>");
neovim_test!(scenarios, jump_search_ctrl_o, "start\nmiddle\ntarget", "/target<CR><C-o>");
neovim_test!(scenarios, jump_mark_ctrl_o, "l1\nl2\nl3", "maG`a");

// ═══════════════════════════════════════════════════════════════════════════════
// COMPLEX NAVIGATION CHAINS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, goto_line_find_change, "l1\nl2\nlet x = old;\nl4", "3Gf=lcw new_val<Esc>");
neovim_test!(scenarios, search_n_cw, "a foo b foo c foo", "/foo<CR>ncwnew<Esc>");
neovim_test!(scenarios, gg_search_edit, "header\n\nfn target() {}", "gg/target<CR>cwmodified<Esc>");
neovim_test!(scenarios, G_k_edit, "l1\nl2\nl3\nl4\nl5", "GkcwNEW<Esc>");
neovim_test!(scenarios, percent_ci, "fn(a + b)", "f(%ci(x, y<Esc>");
neovim_test!(scenarios, w_3_dw, "one two three FOUR five", "3wdw");
neovim_test!(scenarios, dollar_F_change, "hello world test", "$Fwcwnew<Esc>");
neovim_test!(scenarios, caret_cw, "    indented", "^cwnew_word<Esc>");
neovim_test!(scenarios, zero_d_caret, "    indented", "0d^");

// ═══════════════════════════════════════════════════════════════════════════════
// WORD NAVIGATION THEN EDIT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, word_then_change, "one two three", "wciwTWO<Esc>");
neovim_test!(scenarios, word_end_then_append, "hello world", "eaX<Esc>");
neovim_test!(scenarios, back_word_then_delete, "remove this keep", cursor(0, 12), "bdw");
neovim_test!(scenarios, two_words_then_insert, "a b c d", "wwiINSERTED <Esc>");
neovim_test!(scenarios, big_word_then_change, "hello.world test", "WciwTEST<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// COMPLEX CHAINS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, nav_delete_nav_paste, "first second third", "wwdawbP");
neovim_test!(scenarios, search_yank_paste, "hello world target", "/target<CR>yiw0P");
neovim_test!(scenarios, jump_edit_jump_edit, "line1\nline2\nline3", "Gcc1<Esc>ggcc0<Esc>");
neovim_test!(scenarios, find_op_find_op, "a,b,c,d", "f,x;x;x");
