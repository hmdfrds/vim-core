// Scenario fidelity tests: Editing Code
//
// Multi-step workflows for everyday code editing tasks.

// ═══════════════════════════════════════════════════════════════════════════════
// FIX TYPOS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fix_typo_single_char, "recieve", cursor(0, 3), "rece");
neovim_test!(scenarios, fix_typo_with_find, "teh quick brown", "ftxp");
neovim_test!(scenarios, fix_typo_cw, "functoin main()", "ciwfunction<Esc>");
neovim_test!(scenarios, fix_typo_r_middle, "helo world", cursor(0, 2), "rll");
neovim_test!(scenarios, fix_swapped_chars, "adn", cursor(0, 1), "xp");
neovim_test!(scenarios, fix_double_letter, "helllo", cursor(0, 3), "x");
neovim_test!(scenarios, fix_missing_letter, "hllo", cursor(0, 1), "ie<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// RENAME VARIABLES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, rename_var_cw, "let count = 0;", cursor(0, 4), "ciwresult<Esc>");
neovim_test!(scenarios, rename_var_ciw, "let old_name = 0;", cursor(0, 6), "ciwnew_name<Esc>");
neovim_test!(scenarios, rename_func, "fn process()", cursor(0, 3), "ciwhandle<Esc>");
neovim_test!(scenarios, rename_type, "Vec<String>", "ciwHashMap<Esc>");
neovim_test!(scenarios, rename_param, "fn foo(data: i32)", cursor(0, 7), "ciwvalue<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// CHANGE STRINGS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, change_string_content, "let s = \"old\";", cursor(0, 9), "ci\"new<Esc>");
neovim_test!(scenarios, change_single_quote, "let s = 'old';", cursor(0, 9), "ci'new<Esc>");
neovim_test!(scenarios, delete_string_content, "let s = \"remove\";", cursor(0, 9), "di\"");
neovim_test!(scenarios, change_path_string, "let p = \"/old/path\";", cursor(0, 10), "ci\"/new/path<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// LINE OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, swap_two_lines, "line1\nline2", "ddp");
neovim_test!(scenarios, move_line_up, "first\nsecond\nthird", cursor(2, 0), "ddkP");
neovim_test!(scenarios, duplicate_line, "original", "yyp");
neovim_test!(scenarios, delete_line_keep_rest, "keep\ndelete\nkeep", cursor(1, 0), "dd");
neovim_test!(scenarios, blank_line_below, "code();", "o<Esc>");
neovim_test!(scenarios, blank_line_above, "code();", "O<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// ADD / REMOVE CODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, add_semicolon, "let x = 5", "A;<Esc>");
neovim_test!(scenarios, remove_semicolon, "let x = 5;;", "$x");
neovim_test!(scenarios, add_prefix, "fn helper()", "Ipub <Esc>");
neovim_test!(scenarios, add_suffix, "process(data)", "$i, extra<Esc>");
neovim_test!(scenarios, wrap_in_parens, "value", "bi(<Esc>ea)<Esc>");
neovim_test!(scenarios, add_return_type, "fn foo()", "A -> i32<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// COMMENT / UNCOMMENT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, comment_line, "let x = 5;", "I// <Esc>");
neovim_test!(scenarios, uncomment_line, "// let x = 5;", "0d3l");
neovim_test!(scenarios, add_hash_comment, "import os", "I# <Esc>");
neovim_test!(scenarios, uncomment_hash, "# import os", "0d2l");

// ═══════════════════════════════════════════════════════════════════════════════
// ARGUMENT / PARAMETER EDITING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, add_argument, "fn foo(a: i32)", "f)i, b: i32<Esc>");
neovim_test!(scenarios, change_argument, "call(old_arg)", cursor(0, 5), "ciwnew_arg<Esc>");
neovim_test!(scenarios, delete_inner_parens, "call(remove_me)", cursor(0, 5), "di(");
neovim_test!(scenarios, delete_around_parens, "extra(remove) rest", cursor(0, 6), "da(");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATOR CHAINS (multi-step)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cw_then_dot, "old old old", "cwnew<Esc>w.w.");
neovim_test!(scenarios, search_cw_n_dot, "foo bar foo baz foo", "/foo<CR>cwX<Esc>n.n.");
neovim_test!(scenarios, delete_3_words, "one two three four five", "3dw");
neovim_test!(scenarios, delete_to_eol_paste, "keep delete this", "wDjP");
neovim_test!(scenarios, change_inner_bracket, "arr[old_idx]", cursor(0, 4), "ci[new_idx<Esc>");
neovim_test!(scenarios, yank_inner_brace, "fn() { body }", cursor(0, 6), "yi{");
neovim_test!(scenarios, change_to_end, "let x = old_value;", cursor(0, 8), "C new_value;<Esc>");
neovim_test!(scenarios, delete_find_dot, "a.b.c.d", "df..");
neovim_test!(scenarios, code_substitute_global, "old old old", ":s/old/new/g<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// COMBINED WORKFLOWS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fix_and_continue, "teh quick fox", "cwthe<Esc>wwcwfox<Esc>");
neovim_test!(scenarios, delete_add_replace, "const x = old;", "wciwlet<Esc>$bcwnew<Esc>");
neovim_test!(scenarios, dup_3_lines, "template", "yy2p");
neovim_test!(scenarios, move_line_to_end, "move\nkeep1\nkeep2", "ddGp");
neovim_test!(scenarios, copy_word_to_other_line, "source\ntarget", "ywjp");
neovim_test!(scenarios, xp_swap_chars, "ba", "xp");
neovim_test!(scenarios, deep_indent_reduce, "            deep", "<<<<<<<<");
neovim_test!(scenarios, comment_3_lines, "a\nb\nc", "I// <Esc>jI// <Esc>jI// <Esc>");
neovim_test!(scenarios, visual_delete_word, "hello world rest", "viwxwP");
neovim_test!(scenarios, star_search_change, "let data = data;", "*Ncwnew_data<Esc>");
neovim_test!(scenarios, macro_append_semi, "a\nb\nc", "qaA;<Esc>jq2@a");
neovim_test!(scenarios, cc_rewrite, "    old_code();", "ccnew_code();<Esc>");
neovim_test!(scenarios, S_rewrite, "    old_code();", "Snew_code();<Esc>");
neovim_test!(scenarios, visual_line_delete_3, "l1\nl2\nl3\nl4\nl5", "V2jd");
neovim_test!(scenarios, dd_multiple, "l1\nl2\nl3\nl4", "dddddd");
