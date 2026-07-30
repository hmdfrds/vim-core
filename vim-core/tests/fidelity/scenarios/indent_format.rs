// Scenario fidelity tests: Indent, Format, Whitespace
//
// Real-world indentation, formatting, and whitespace management.

// ═══════════════════════════════════════════════════════════════════════════════
// INDENT OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, indent_single, "hello", ">>");
neovim_test!(scenarios, indent_twice, "hello", ">>>>");
neovim_test!(scenarios, outdent_single, "        hello", "<<");
neovim_test!(scenarios, outdent_twice, "        hello", "<<<<");
neovim_test!(scenarios, fmt_indent_motion_j, "l1\nl2\nl3", ">j");
neovim_test!(scenarios, indent_motion_2j, "l1\nl2\nl3\nl4", ">2j");
neovim_test!(scenarios, outdent_motion_j, "    l1\n    l2\n    l3", "<j");
neovim_test!(scenarios, fmt_indent_paragraph, "a\nb\n\nc\nd", ">}");
neovim_test!(scenarios, indent_gg, "l1\nl2\nl3", cursor(2, 0), ">gg");
neovim_test!(scenarios, indent_G, "l1\nl2\nl3", ">G");

// ═══════════════════════════════════════════════════════════════════════════════
// INDENT + CASE OPERATOR COMBOS (oracle regression tests)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, indent_then_guu, "HELLO", ">>guu");
neovim_test!(scenarios, indent_then_gUU, "hello", ">>gUU");
neovim_test!(scenarios, indent_then_case_multiline, "HELLO\nWORLD", ">>guu");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL INDENT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, visual_indent_2_lines, "a\nb\nc", "Vj>");
neovim_test!(scenarios, visual_indent_all, "a\nb\nc", "VG>");
neovim_test!(scenarios, visual_outdent_2, "    a\n    b\n    c", "Vj<");
neovim_test!(scenarios, visual_indent_3x, "a\nb\nc", "Vj>>>");
neovim_test!(scenarios, visual_indent_block, "a\nb\nc", "<C-v>2j>");
neovim_test!(scenarios, visual_outdent_block, "    a\n    b\n    c", "<C-v>2j<");
// Block indent with selection NOT starting at column 0 — verifies that > indents
// at line start (col 0), not at the block's left column.
neovim_test!(scenarios, visual_indent_block_mid_col, "hello\nworld\nfinal", "l<C-v>2jl>");
neovim_test!(scenarios, visual_outdent_block_mid_col, "    hello\n    world\n    final", "l<C-v>2jl<");

// ═══════════════════════════════════════════════════════════════════════════════
// DOT REPEAT INDENT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, indent_dot, "a\nb\nc", ">>j.j.");
neovim_test!(scenarios, outdent_dot, "    a\n    b\n    c", "<<j.j.");
neovim_test!(scenarios, indent_j_dot, "a\nb\nc\nd\ne", ">jjj.");

// ═══════════════════════════════════════════════════════════════════════════════
// FORMAT (gq)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fmt_gqq_basic, "hello world", "gqq");
neovim_test!(scenarios, fmt_gq_paragraph, "hello world foo bar baz qux\n\nnew para", "gq}");
neovim_test!(scenarios, gqj_two_lines, "line one\nline two", "gqj");

// ═══════════════════════════════════════════════════════════════════════════════
// WHITESPACE MANAGEMENT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, delete_trailing_spaces, "hello   ", "$diw");
neovim_test!(scenarios, strip_leading_space, "   hello", "0dw");
neovim_test!(scenarios, remove_blank_line, "above\n\nbelow", cursor(1, 0), "dd");
neovim_test!(scenarios, add_blank_line, "above\nbelow", "o<Esc>");
neovim_test!(scenarios, collapse_blank_lines, "a\n\n\n\nb", cursor(1, 0), "2dd");
neovim_test!(scenarios, tab_to_spaces, "hello\tworld", "f\tr    <Esc>");
