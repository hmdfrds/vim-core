// Scenario fidelity tests: Search & Replace Workflows
//
// Multi-step workflows using search and replace patterns.

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH AND CHANGE (n. PATTERN)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, search_change_next, "foo bar foo baz foo", "/foo<CR>ciwbar<Esc>n.");
neovim_test!(scenarios, search_delete_all, "aa bb aa cc aa", "/aa<CR>daw;.");
neovim_test!(scenarios, star_change_repeat, "old val old val", "*ciwreplaced<Esc>n.");
neovim_test!(scenarios, search_change_one, "find me here", "/me<CR>cwus<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// FIND AND OPERATE REPEATEDLY
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, delete_all_commas, "a,b,c,d", "f,x;x;x");
neovim_test!(scenarios, find_delete_semicolons, "a;b;c;d", "f;x;x;x");
neovim_test!(scenarios, find_replace_dots, "a.b.c", "f.r_f.r_");
neovim_test!(scenarios, find_change_repeat, "x=1 y=2 z=3", "f=r:;r:;r:");

// ═══════════════════════════════════════════════════════════════════════════════
// MACRO BASED SEARCH REPLACE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, macro_replace_words, "aa bb aa bb", "qaciwXX<Esc>wq2@a");
neovim_test!(scenarios, macro_append_many, "line1\nline2\nline3", "qaA!<Esc>jq2@a");
neovim_test!(scenarios, macro_comment_lines, "a\nb\nc", "qaI// <Esc>jq2@a");
neovim_test!(scenarios, macro_delete_first_word, "rm me\nrm me\nrm me", "qadwjq2@a");

// ═══════════════════════════════════════════════════════════════════════════════
// SUBSTITUTION COMMAND
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, substitute_first, "old and old", ":s/old/new<CR>");
neovim_test!(scenarios, substitute_global, "old and old", ":s/old/new/g<CR>");
neovim_test!(scenarios, substitute_whole_word, "foo foobar foo", ":s/foo/bar<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// SELECTIVE EDIT WITH SEARCH
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, search_skip_change, "aa bb aa cc aa", "/aa<CR>nciwZZ<Esc>");
neovim_test!(scenarios, backward_search_edit, "first target second", cursor(0, 18), "?target<CR>cwfound<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// BACKWARD SEARCH PATTERNS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, backward_search_delete, "a b target c", cursor(0, 11), "?target<CR>daw");
neovim_test!(scenarios, backward_N_forward, "foo bar foo baz foo", "/foo<CR>nNcwX<Esc>");
neovim_test!(scenarios, backward_search_paste, "target rest", cursor(0, 10), "?target<CR>ywGp");

// ═══════════════════════════════════════════════════════════════════════════════
// STAR / HASH SEARCH
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, star_navigate, "foo bar foo baz foo", "*n");
neovim_test!(scenarios, hash_navigate, "foo bar foo baz foo", cursor(0, 16), "#n");
neovim_test!(scenarios, star_cw, "old bar old baz old", "*Ncwreplaced<Esc>");
neovim_test!(scenarios, star_dn, "rm bar rm baz rm", "*Ndaw");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH + REGISTER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, sr_search_yank_paste, "start target end", "/target<CR>yw$p");
neovim_test!(scenarios, search_delete_paste, "start remove end", "/remove<CR>daw$p");

// ═══════════════════════════════════════════════════════════════════════════════
// ADVANCED SUBSTITUTION
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, sub_all_lines_global, "old\nold\nold", ":%s/old/new/g<CR>");
neovim_test!(scenarios, sr_sub_range, "aa\nbb\ncc\ndd", ":2,3s/./X/g<CR>");
neovim_test!(scenarios, sub_regex_dot, "a.b.c", ":s/\\./,/g<CR>");
neovim_test!(scenarios, sr_sub_case_insensitive, "Hello HELLO hello", ":s/hello/HI/gi<CR>");
neovim_test!(scenarios, sub_delete_pattern, "remove_this rest", ":s/remove_this //g<CR>");
neovim_test!(scenarios, sub_ampersand, "word", ":s/word/&_extra<CR>");
neovim_test!(scenarios, sub_empty_to_prefix, "line", ":s/^/>> <CR>");
