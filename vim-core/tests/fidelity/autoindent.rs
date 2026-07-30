// Auto-indent operator fidelity tests (= operator).
//
// The = operator auto-indents lines, similar to gg=G for whole-file reindent.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC == (INDENT CURRENT LINE)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(autoindent, equal_equal_basic, "hello", "==");
neovim_test!(autoindent, equal_equal_indented, "    hello", "==");
neovim_test!(autoindent, equal_equal_deep_indent, "            deep", "==");
neovim_test!(autoindent, equal_equal_no_indent, "hello", "==");
neovim_test!(autoindent, equal_equal_tabs, "\thello", "==");
neovim_test!(autoindent, equal_equal_mixed, "\t  hello", "==");
neovim_test!(autoindent, equal_equal_empty, "", "==");
neovim_test!(autoindent, equal_equal_blank_line, "hello\n\nworld", cursor(1, 0), "==");

// ═══════════════════════════════════════════════════════════════════════════════
// = WITH MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(autoindent, equal_j, "  hello\n  world", "=j");
neovim_test!(autoindent, equal_k, "  hello\n  world", cursor(1, 0), "=k");
neovim_test!(autoindent, equal_G, "  hello\n  world\n  foo", "=G");
neovim_test!(autoindent, equal_gg, "  hello\n  world\n  foo", cursor(2, 0), "=gg");
neovim_test!(autoindent, equal_2j, "  l1\n  l2\n  l3", "=2j");
neovim_test!(autoindent, equal_w, "  hello world", "=w");
neovim_test!(autoindent, equal_dollar, "  hello world", "=$");
neovim_test!(autoindent, equal_percent, "  if {\n    body\n  }", "=%");

// ═══════════════════════════════════════════════════════════════════════════════
// = WITH TEXT OBJECTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(autoindent, equal_ip, "  hello\n  world\n\nother", "=ip");
neovim_test!(autoindent, equal_ap, "  hello\n  world\n\nother", "=ap");
neovim_test!(autoindent, equal_i_brace, "{\n  hello\n  world\n}", cursor(1, 0), "=i{");
neovim_test!(autoindent, equal_a_brace, "{\n  hello\n  world\n}", cursor(1, 0), "=a{");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL MODE =
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(autoindent, visual_equal, "  hello\n  world", "Vj=");
neovim_test!(autoindent, visual_equal_single, "    hello", "V=");
neovim_test!(autoindent, visual_equal_block, "    hello\n    world\n    foo", "Vjj=");
neovim_test!(autoindent, visual_equal_mixed, "hello\n    world\n        deep", "Vjj=");

// ═══════════════════════════════════════════════════════════════════════════════
// gg=G (WHOLE FILE REINDENT)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(autoindent, gg_equal_G, "  hello\n    world\n      deep", "gg=G");
neovim_test!(autoindent, gg_equal_G_code, "if true {\nhello\nworld\n}", "gg=G");
neovim_test!(autoindent, gg_equal_G_nested, "fn main() {\nif true {\nbody\n}\n}", "gg=G");

// ═══════════════════════════════════════════════════════════════════════════════
// = WITH COUNT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(autoindent, count_2_equal_equal, "  hello\n  world\n  foo", "2==");
neovim_test!(autoindent, count_3_equal_equal, "  a\n  b\n  c\n  d", "3==");

// ═══════════════════════════════════════════════════════════════════════════════
// = DOT REPEAT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(autoindent, equal_dot_repeat, "  hello\n  world\n  foo", "==j.");
neovim_test!(autoindent, equal_j_dot, "  l1\n  l2\n  l3\n  l4", "=jjj.");
