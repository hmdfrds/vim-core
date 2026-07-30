// Scenario fidelity tests: Find/Till/Repeat Editing
//
// f/F/t/T with ;/, in real editing patterns.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC FIND-EDIT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ftr_find_delete_to, "hello world foo", "dfo");
neovim_test!(scenarios, ftr_find_change_to, "hello world foo", "cfwNEW<Esc>");
neovim_test!(scenarios, ftr_find_yank_to, "hello world foo", "yfo$p");
neovim_test!(scenarios, till_delete_to, "hello world foo", "dtw");
neovim_test!(scenarios, till_change_to, "hello world foo", "ctwNEW<Esc>");
neovim_test!(scenarios, till_yank_to, "hello world foo", "ytw$p");

// ═══════════════════════════════════════════════════════════════════════════════
// BACKWARD FIND-EDIT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, Fb_delete, "hello world", cursor(0, 10), "dFw");
neovim_test!(scenarios, Fb_change, "hello world", cursor(0, 10), "cFwnew<Esc>");
neovim_test!(scenarios, Tb_delete, "hello world", cursor(0, 10), "dTw");
neovim_test!(scenarios, Tb_change, "hello world", cursor(0, 10), "cTwnew<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// REPEAT WITH SEMICOLON
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, find_semi_to_third, "a.b.c.d.e", "f.;;");
neovim_test!(scenarios, find_semi_delete, "a,b,c,d", "f,;d;");
neovim_test!(scenarios, find_semi_change, "a=b=c=d", "f=;ci=X<Esc>");
neovim_test!(scenarios, till_semi_skip, "abcabc", "ta;;");
neovim_test!(scenarios, find_semi_x, "x-y-z-w", "f-;x");
neovim_test!(scenarios, find_semi_2x, "a.b.c.d.e", "f.;df.");

// ═══════════════════════════════════════════════════════════════════════════════
// REPEAT WITH COMMA (REVERSE)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, find_comma_reverse, "a.b.c.d.e", cursor(0, 8), "Ff,,");
neovim_test!(scenarios, find_fwd_comma_back, "a,b,c,d", "f,$,");
neovim_test!(scenarios, find_comma_delete, "a b c d", "fb;;d,");

// ═══════════════════════════════════════════════════════════════════════════════
// MULTI-CHAR FIND CHAINS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fwd_two_finds, "a(b)c", "f(ldt)");
neovim_test!(scenarios, find_paren_di, "fn(a, b)", "f(di(");
neovim_test!(scenarios, find_bracket_ci, "arr[idx]", "f[ci[0<Esc>");
neovim_test!(scenarios, find_quote_di, "say(\"hello\")", "f\"di\"");
neovim_test!(scenarios, find_dot_cw, "obj.method.call", "f.;lcwresult<Esc>");
neovim_test!(scenarios, find_colon_dt, "key: value", "f:dt ");
neovim_test!(scenarios, find_eq_ct, "x = old", "f lctold<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// REAL-WORLD FIND PATTERNS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, find_comma_delete_arg, "fn(a, b, c)", cursor(0, 4), "df,");
neovim_test!(scenarios, till_paren_yank, "process(input)", "yt(");
neovim_test!(scenarios, find_semi_change_val, "let x = 42;", "f4ct;99<Esc>");
neovim_test!(scenarios, find_arrow_cf, "a -> b -> c", "cf>X<Esc>");
neovim_test!(scenarios, till_brace_d, "if cond { body }", "dt{");
