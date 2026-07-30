// Unmatched bracket motion fidelity tests ([{, ]}, [(, ])).

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC [{ and ]}
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(bracket_motions, prev_brace_basic, "{\n  foo\n  bar\n}", cursor(1, 2), "[{");
neovim_test!(bracket_motions, next_brace_basic, "{\n  foo\n  bar\n}", cursor(1, 2), "]}");
neovim_test!(bracket_motions, prev_brace_nested, "{\n  {\n    foo\n  }\n}", cursor(2, 4), "[{");
neovim_test!(bracket_motions, next_brace_nested, "{\n  {\n    foo\n  }\n}", cursor(2, 4), "]}");
neovim_test!(bracket_motions, prev_brace_no_match, "foo bar", "[{");

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC [( and ])
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(bracket_motions, prev_paren_basic, "(\n  foo\n  bar\n)", cursor(1, 2), "[(");
neovim_test!(bracket_motions, next_paren_basic, "(\n  foo\n  bar\n)", cursor(1, 2), "])");
neovim_test!(bracket_motions, prev_paren_nested, "(\n  (\n    foo\n  )\n)", cursor(2, 4), "[(");
neovim_test!(bracket_motions, next_paren_nested, "(\n  (\n    foo\n  )\n)", cursor(2, 4), "])");

// ═══════════════════════════════════════════════════════════════════════════════
// WITH OPERATORS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(bracket_motions, delete_to_prev_brace, "{\n  foo\n  bar\n}", cursor(2, 2), "d[{");
neovim_test!(bracket_motions, yank_to_next_brace, "{\n  foo\n  bar\n}", cursor(1, 2), "y]}p");
neovim_test!(bracket_motions, change_to_prev_paren, "(foo bar)", cursor(0, 5), "c[(X<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// WITH COUNT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(bracket_motions, prev_brace_count_2, "{\n  {\n    foo\n  }\n}", cursor(2, 4), "2[{");
neovim_test!(bracket_motions, next_brace_count_2, "{\n  {\n    foo\n  }\n}", cursor(2, 4), "2]}");
neovim_test!(bracket_motions, prev_paren_count_2, "(\n  (\n    foo\n  )\n)", cursor(2, 4), "2[(");
neovim_test!(bracket_motions, next_paren_count_2, "(\n  (\n    foo\n  )\n)", cursor(2, 4), "2])");

// ═══════════════════════════════════════════════════════════════════════════════
// DEEPER NESTING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(bracket_motions, prev_brace_deep, "{\n  {\n    {\n      foo\n    }\n  }\n}", cursor(3, 6), "[{");
neovim_test!(bracket_motions, next_brace_deep, "{\n  {\n    {\n      foo\n    }\n  }\n}", cursor(3, 6), "]}");
neovim_test!(bracket_motions, prev_brace_from_outer, "{\n  {\n    foo\n  }\n}", cursor(4, 0), "[{");
neovim_test!(bracket_motions, next_brace_from_top, "{\n  {\n    foo\n  }\n}", "]}");

// ═══════════════════════════════════════════════════════════════════════════════
// SAME-LINE BRACKETS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(bracket_motions, prev_brace_same_line, "{ foo }", cursor(0, 3), "[{");
neovim_test!(bracket_motions, next_brace_same_line, "{ foo }", cursor(0, 3), "]}");
neovim_test!(bracket_motions, prev_paren_same_line, "( foo )", cursor(0, 3), "[(");
neovim_test!(bracket_motions, next_paren_same_line, "( foo )", cursor(0, 3), "])");

// ═══════════════════════════════════════════════════════════════════════════════
// MIXED BRACKET TYPES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(bracket_motions, paren_inside_brace, "{\n  (foo)\n}", cursor(1, 3), "[{");
neovim_test!(bracket_motions, brace_inside_paren, "(\n  {foo}\n)", cursor(1, 3), "[(");

// ═══════════════════════════════════════════════════════════════════════════════
// MORE OPERATORS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(bracket_motions, visual_to_prev_brace, "{\n  foo\n  bar\n}", cursor(2, 2), "v[{d");
neovim_test!(bracket_motions, visual_to_next_brace, "{\n  foo\n  bar\n}", cursor(1, 2), "v]}d");
neovim_test!(bracket_motions, yank_to_prev_paren, "(\n  foo\n  bar\n)", cursor(2, 2), "y[(p");
neovim_test!(bracket_motions, delete_to_next_paren, "(\n  foo\n  bar\n)", cursor(1, 2), "d])");
neovim_test!(bracket_motions, change_to_next_brace, "{\n  foo\n  bar\n}", cursor(1, 2), "c]}X<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(bracket_motions, prev_brace_at_brace, "{\n  foo\n}", cursor(0, 0), "[{");
neovim_test!(bracket_motions, next_brace_at_brace, "{\n  foo\n}", cursor(2, 0), "]}");
neovim_test!(bracket_motions, no_match_paren, "foo bar", "[(");
neovim_test!(bracket_motions, no_match_next_paren, "foo bar", "])");
neovim_test!(bracket_motions, empty_braces, "{ }", cursor(0, 1), "[{");
neovim_test!(bracket_motions, empty_parens, "( )", cursor(0, 1), "[(");
neovim_test!(bracket_motions, string_with_braces, "\"{\"\nfoo\n\"}\"", cursor(1, 0), "[{");
