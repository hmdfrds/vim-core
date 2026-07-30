// Scenario fidelity tests: Advanced Visual Mode
//
// gv, o/O swap, visual+textobj, block operations, visual transformations.

// ═══════════════════════════════════════════════════════════════════════════════
// gv (RESELECT)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, va_gv_after_yank, "hello world", "viwygvd");
neovim_test!(scenarios, gv_after_indent, "hello\nworld", "Vj>gv>");
neovim_test!(scenarios, gv_after_case, "hello", "viwgUgvgu");
neovim_test!(scenarios, va_gv_after_delete, "aaa bbb ccc", "viwdgvd");

// ═══════════════════════════════════════════════════════════════════════════════
// o/O (SWAP SELECTION ENDS)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, visual_o_extend, "hello world", "vwold");
neovim_test!(scenarios, visual_o_shrink, "abcdefgh", "v$ohd");
neovim_test!(scenarios, visual_line_o, "l1\nl2\nl3\nl4", "Vjod");
neovim_test!(scenarios, visual_block_O, "abc\nabc\nabc", "<C-v>2jlOld");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL + TEXT OBJECTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, visual_iw_delete, "hello world foo", cursor(0, 6), "viwd");
neovim_test!(scenarios, visual_aw_delete, "hello world foo", cursor(0, 6), "vawd");
neovim_test!(scenarios, visual_i_paren, "fn(a, b)", cursor(0, 3), "vi(d");
neovim_test!(scenarios, visual_a_paren, "fn(a, b)", cursor(0, 3), "va(d");
neovim_test!(scenarios, visual_i_brace, "{ body }", cursor(0, 2), "vi{d");
neovim_test!(scenarios, visual_a_brace, "{ body }", cursor(0, 2), "va{d");
neovim_test!(scenarios, visual_i_bracket, "[1, 2, 3]", cursor(0, 1), "vi[d");
neovim_test!(scenarios, visual_i_quote, "\"hello\"", cursor(0, 1), "vi\"d");
neovim_test!(scenarios, visual_a_quote, "\"hello\" rest", cursor(0, 1), "va\"d");
neovim_test!(scenarios, visual_i_single, "'hello'", cursor(0, 1), "vi'd");
neovim_test!(scenarios, visual_ip, "para1\n\npara2", "vipd");
neovim_test!(scenarios, visual_ap, "para1\n\npara2", "vapd");
neovim_test!(scenarios, visual_is, "One. Two. Three.", "visd");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL LINE OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, visual_line_move_down, "l1\nl2\nl3", "Vjd");
neovim_test!(scenarios, visual_line_yank_paste, "l1\nl2\nl3", "VyGp");
neovim_test!(scenarios, visual_line_join, "l1\nl2\nl3", "VjJ");
neovim_test!(scenarios, visual_line_change, "l1\nl2\nl3", "VjcNEW<Esc>");
neovim_test!(scenarios, visual_line_indent, "l1\nl2\nl3", "Vj>>");
neovim_test!(scenarios, visual_line_case, "hello world", "VgU");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL BLOCK OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, va_vblock_delete_col, "abc\nabc\nabc", "<C-v>2jd");
neovim_test!(scenarios, vblock_delete_2col, "abcd\nabcd\nabcd", "<C-v>2jld");
neovim_test!(scenarios, vblock_insert, "aaa\nbbb\nccc", "<C-v>2jI# <Esc>");
neovim_test!(scenarios, vblock_append, "aaa\nbbb\nccc", "<C-v>2j$A;<Esc>");
neovim_test!(scenarios, vblock_change, "aaa\nbbb\nccc", "<C-v>2jcX<Esc>");
neovim_test!(scenarios, vblock_replace, "aaa\naaa\naaa", "<C-v>2jlrX");
neovim_test!(scenarios, vblock_case, "aaa\naaa\naaa", "<C-v>2jl~");

// ═══════════════════════════════════════════════════════════════════════════════
// VISUAL TO NORMAL WORKFLOWS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, select_inner_then_upper, "hello world", cursor(0, 6), "viwgU");
neovim_test!(scenarios, select_yank_move_paste, "keep copy\nmove here", "viwyjwP");
neovim_test!(scenarios, visual_surround_like, "hello world", cursor(0, 6), "viwdi(<Esc>hP");
