// Miscellaneous command fidelity tests.
//
// Features that don't fit neatly into other categories:
// g?, gm, gI, gw, ZZ, ZQ, &, g&, Ctrl-G, ga, Ctrl-^

// ═══════════════════════════════════════════════════════════════════════════════
// g? — ROT13 OPERATOR
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(misc_commands, rot13_word, "hello", "g?w");
neovim_test!(misc_commands, rot13_line, "hello world", "g?g?");
neovim_test!(misc_commands, rot13_double, "hello", "g??");
neovim_test!(misc_commands, rot13_motion_e, "hello world", "g?e");
neovim_test!(misc_commands, rot13_motion_dollar, "hello world", "g?$");
neovim_test!(misc_commands, rot13_visual, "hello world", "vwg?");
neovim_test!(misc_commands, rot13_visual_line, "hello\nworld", "Vjg?");
neovim_test!(misc_commands, rot13_numbers_unchanged, "abc 123 def", "g?$");
neovim_test!(misc_commands, rot13_mixed_case, "Hello World", "g?$");
neovim_test!(misc_commands, rot13_roundtrip, "hello", "g?$0g?$");
neovim_test!(misc_commands, rot13_dot_repeat, "hello\nworld", "g?$j.");
neovim_test!(misc_commands, rot13_count, "hello\nworld\nfoo", "2g?g?");
neovim_test!(misc_commands, rot13_iw, "hello world", "g?iw");

// ═══════════════════════════════════════════════════════════════════════════════
// gI — INSERT AT COLUMN 1 (NOT FIRST NON-BLANK)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(misc_commands, gI_basic, "hello", "gIX<Esc>");
neovim_test!(misc_commands, gI_indented, "    hello", "gIX<Esc>");
neovim_test!(misc_commands, gI_empty, "", "gIhello<Esc>");
neovim_test!(misc_commands, gI_dot_repeat, "    hello\n    world", "gI// <Esc>j.");
neovim_test!(misc_commands, gI_vs_I, "    hello", "gIX<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// gw — FORMAT KEEPING CURSOR POSITION
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(misc_commands, gw_word, "hello world", "gww");
neovim_test!(misc_commands, gw_line, "hello world foo bar", "gwgw");
neovim_test!(misc_commands, gw_visual, "hello world\nfoo bar", "Vjgw");
neovim_test!(misc_commands, gw_j, "hello world\nfoo bar", "gwj");

// ═══════════════════════════════════════════════════════════════════════════════
// gm — MIDDLE OF SCREEN LINE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(misc_commands, gm_basic, "hello world foo bar", "gm");
neovim_test!(misc_commands, gm_short_line, "hi", "gm");
neovim_test!(misc_commands, gm_empty, "", "gm");

// ═══════════════════════════════════════════════════════════════════════════════
// ZZ / ZQ — QUICK SAVE/QUIT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(misc_commands, ZZ_basic, "hello", "ZZ");
neovim_test!(misc_commands, ZQ_basic, "hello", "ZQ");

// ═══════════════════════════════════════════════════════════════════════════════
// & / g& — REPEAT LAST SUBSTITUTION
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(misc_commands, ampersand_basic, "old and old\nold and old", ":s/old/new<CR>j&");
neovim_test!(misc_commands, ampersand_global, "old old\nold old", ":s/old/new/g<CR>j&");
neovim_test!(misc_commands, g_ampersand, "old old\nold old", ":s/old/new/g<CR>g&");
neovim_test!(misc_commands, ampersand_no_prev_sub, "hello", "&");
neovim_test!(misc_commands, ampersand_dot_repeat, "old\nold\nold", ":s/old/new<CR>j.j.");

// ═══════════════════════════════════════════════════════════════════════════════
// Ctrl-G — FILE INFO
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(misc_commands, ctrl_g_basic, "hello world", "<C-g>");
neovim_test!(misc_commands, ctrl_g_multiline, "line1\nline2\nline3", "<C-g>");
neovim_test!(misc_commands, ctrl_g_at_end, "hello\nworld", cursor(1, 0), "<C-g>");

// ═══════════════════════════════════════════════════════════════════════════════
// ga — SHOW ASCII VALUE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(misc_commands, ga_basic, "A", "ga");
neovim_test!(misc_commands, ga_space, " hello", "ga");
neovim_test!(misc_commands, ga_number, "0", "ga");
neovim_test!(misc_commands, ga_unicode, "日", "ga");

// ═══════════════════════════════════════════════════════════════════════════════
// Ctrl-^ — ALTERNATE FILE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(misc_commands, ctrl_caret_basic, "hello", "<C-^>");

// ═══════════════════════════════════════════════════════════════════════════════
// Ctrl-L — REDRAW SCREEN
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(misc_commands, ctrl_l_basic, "hello world", "<C-l>");

// ═══════════════════════════════════════════════════════════════════════════════
// g; / g, — CHANGELIST NAVIGATION
// ═══════════════════════════════════════════════════════════════════════════════

// g; already tested in motions.rs but these add more scenarios
neovim_test!(misc_commands, g_semicolon_after_insert, "hello\nworld", "iX<Esc>jiY<Esc>g;");
neovim_test!(misc_commands, g_comma_forward, "hello\nworld", "iX<Esc>jiY<Esc>g;g,");
neovim_test!(misc_commands, g_semicolon_multiple, "a\nb\nc", "iX<Esc>jiY<Esc>jiZ<Esc>g;g;");
neovim_test!(misc_commands, misc_g_semicolon_no_changes, "hello", "g;");
neovim_test!(misc_commands, misc_g_comma_no_forward, "hello", "g,");

// ═══════════════════════════════════════════════════════════════════════════════
// gn / gN — SEARCH AND SELECT (more coverage)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(misc_commands, gn_basic, "hello world hello", "/hello<CR>gn");
neovim_test!(misc_commands, gn_change, "hello world hello", "/hello<CR>cgnX<Esc>");
neovim_test!(misc_commands, gn_delete, "hello world hello", "/hello<CR>dgn");
neovim_test!(misc_commands, gn_dot_repeat, "hello world hello end hello", "/hello<CR>cgnX<Esc>...");
neovim_test!(misc_commands, gN_basic, "hello world hello", cursor(0, 12), "?hello<CR>gN");
neovim_test!(misc_commands, gN_change, "hello world hello", cursor(0, 12), "?hello<CR>cgNX<Esc>");
neovim_test!(misc_commands, gn_with_star, "hello world hello end hello", "*gn");
neovim_test!(misc_commands, gn_yank, "hello world hello", "/hello<CR>ygn$p");

// ═══════════════════════════════════════════════════════════════════════════════
// g_ — LAST NON-BLANK CHARACTER
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(misc_commands, misc_g_underscore_trailing, "hello   ", "g_");
neovim_test!(misc_commands, misc_g_underscore_plain, "hello", "g_");
neovim_test!(misc_commands, misc_g_underscore_count, "l1\nl2   \nl3", "2g_");
neovim_test!(misc_commands, misc_g_underscore_delete, "hello   ", "dg_");
neovim_test!(misc_commands, misc_g_underscore_change, "hello   ", "cg_X<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// cr* — ABOLISH CASE COERCION
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(misc_commands, crs_camel_to_snake, "helloWorld", "crs");
neovim_test!(misc_commands, crc_snake_to_camel, "hello_world", "crc");
neovim_test!(misc_commands, crm_to_mixed_case, "hello_world", "crm");
neovim_test!(misc_commands, cru_to_upper_snake, "hello_world", "cru");
neovim_test!(misc_commands, cr_dash_to_kebab, "hello_world", "cr-");
neovim_test!(misc_commands, crs_from_camel_mid, "helloWorldFoo", cursor(0, 5), "crs");

// ═══════════════════════════════════════════════════════════════════════════════
// q: — COMMAND WINDOW
// ═══════════════════════════════════════════════════════════════════════════════

// q: emits an OpenCommandWindow effect which is a host-side UI concern.
// The test verifies the key is consumed and text/cursor state is unchanged.
neovim_test!(misc_commands, q_colon_basic, "hello", "q:");

// ═══════════════════════════════════════════════════════════════════════════════
// WHICHWRAP (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// h wraps to previous line end with whichwrap=h,l
neovim_test!(misc_commands, h_whichwrap_wraps, "abc\ndef", cursor(1, 0), ":set whichwrap=h,l<CR>h");

// l wraps to next line start with whichwrap=h,l
neovim_test!(misc_commands, l_whichwrap_wraps, "abc\ndef", cursor(0, 2), ":set whichwrap=h,l<CR>l");

// h wraps over an empty line with whichwrap=h,l
neovim_test!(misc_commands, h_whichwrap_over_empty, "abc\n\ndef", cursor(2, 0), ":set whichwrap=h,l<CR>hh");

// ═══════════════════════════════════════════════════════════════════════════════
// ; RANGE SEPARATOR (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// :3;+1d — semicolon sets cursor to line 3, +1 is relative
neovim_test!(misc_commands, semicolon_range_basic, "l1\nl2\nl3\nl4\nl5", ":3;+1d<CR>");

// ;-range differs from ,-range for same addresses
neovim_test!(misc_commands, semicolon_vs_comma, "l1\nl2\nl3\nl4\nl5", cursor(1, 0), ":3;+1d<CR>");

// :1;$d — semicolon with $ (last line)
neovim_test!(misc_commands, semicolon_to_last, "l1\nl2\nl3", ":1;$d<CR>");

// ═══════════════════════════════════════════════════════════════════════════════
// :GLOBAL COMMAND (migrated from neovim_fidelity)
// ═══════════════════════════════════════════════════════════════════════════════

// :g/aaa/s/aaa/REPLACED/ — substitute on matching lines
neovim_test!(misc_commands, global_sub_replace, "aaa\nbbb\naaa\nccc", ":g/aaa/s/aaa/REPLACED/<CR>");

// :g/---/join — join matching lines with separator pattern
neovim_test!(misc_commands, global_join_separator, "---\naaa\n---\nbbb\nccc", ":g/---/join<CR>");

// :g/^$/d — delete empty lines
neovim_test!(misc_commands, global_delete_empty, "a\n\nb\n\nc", ":g/^$/d<CR>");

// :g/remove/d — delete matching lines
neovim_test!(misc_commands, global_delete_pattern, "keep\nremove\nkeep\nremove", ":g/remove/d<CR>");
