// Scenario fidelity tests: Multi-line Operations
//
// Multi-step workflows across multiple lines.

// ═══════════════════════════════════════════════════════════════════════════════
// BLOCK DELETE / CHANGE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, delete_two_lines, "l1\nl2\nl3\nl4", cursor(1, 0), "2dd");
neovim_test!(scenarios, delete_visual_lines, "l1\nl2\nl3\nl4", "Vjd");
neovim_test!(scenarios, delete_to_end, "l1\nl2\nl3\nl4", cursor(1, 0), "dG");
neovim_test!(scenarios, delete_to_start, "l1\nl2\nl3\nl4", cursor(2, 0), "dgg");
neovim_test!(scenarios, change_two_lines, "l1\nl2\nl3", "2ccnew<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// MOVE LINES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, move_line_down, "first\nsecond\nthird", "ddp");
neovim_test!(scenarios, move_line_to_top, "a\nb\nc", cursor(2, 0), "ddggP");
neovim_test!(scenarios, move_line_to_bottom, "a\nb\nc", "ddGp");
neovim_test!(scenarios, swap_adjacent, "alpha\nbeta", "ddp");

// ═══════════════════════════════════════════════════════════════════════════════
// COPY LINES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, copy_paste_below, "template", "yyp");
neovim_test!(scenarios, copy_paste_above, "content", "yyP");
neovim_test!(scenarios, copy_multiple_lines, "l1\nl2\nl3", "2yyGp");
neovim_test!(scenarios, copy_and_modify, "fn old() {}", "yypwcwnew<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL BLOCK / INDENT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, indent_block, "a\nb\nc", "Vj>");
neovim_test!(scenarios, outdent_block, "    a\n    b\n    c", "Vj<");
neovim_test!(scenarios, visual_delete_block, "abc\ndef\nghi", "<C-v>jld");
neovim_test!(scenarios, visual_line_yank, "l1\nl2\nl3", "Vjy$p");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL BLOCK INSERT/APPEND
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, vblock_insert_prefix, "aaa\nbbb\nccc", "<C-v>2jI# <Esc>");
neovim_test!(scenarios, vblock_append_suffix, "aaa\nbbb\nccc", "<C-v>2j$A;<Esc>");
neovim_test!(scenarios, vblock_delete_col, "xabc\nxdef\nxghi", "<C-v>2jx");
neovim_test!(scenarios, vblock_replace_col, "abc\nabc\nabc", "<C-v>2jrX");

// ═══════════════════════════════════════════════════════════════════════════════
// REORDER LINES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, reverse_3_lines, "c\nb\na", "ddpjddkkP");
neovim_test!(scenarios, move_last_to_first, "b\nc\na", "GddggP");
neovim_test!(scenarios, swap_3, "third\nsecond\nfirst", "ddGpggddGp");
neovim_test!(scenarios, move_middle, "a\nc\nb", cursor(2, 0), "ddkP");

// ═══════════════════════════════════════════════════════════════════════════════
// MULTI-LINE YANK / PUT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, yank_4_lines, "a\nb\nc\nd\ne", "4yyGp");
neovim_test!(scenarios, yank_to_line, "l1\nl2\nl3\nl4", "yGGp");
neovim_test!(scenarios, visual_line_yank_put, "a\nb\nc", "Vjy2Gp");
neovim_test!(scenarios, yank_para_paste, "p1\np2\n\np3", "y}Gp");

// ═══════════════════════════════════════════════════════════════════════════════
// JOIN MULTI
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, join_all, "a\nb\nc\nd", "JJJJ");
neovim_test!(scenarios, join_count_4, "a\nb\nc\nd", "4J");
neovim_test!(scenarios, visual_join, "a\nb\nc", "VGJ");

