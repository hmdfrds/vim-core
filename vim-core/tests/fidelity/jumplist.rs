// JumpList fidelity tests for vim-core.
//
// Tests for jump list navigation with Ctrl-O and Ctrl-I.
// Jump list stores cursor positions for large jumps.

// ═══════════════════════════════════════════════════════════════════════════════
// BASIC JUMP MOTIONS (Push to jump list)
// ═══════════════════════════════════════════════════════════════════════════════

// These motions should push to the jump list before executing
neovim_test!(jumplist, jump_gg, "line1\nline2\nline3\nline4", "Ggg");
neovim_test!(jumplist, jump_G, "line1\nline2\nline3", "G");
neovim_test!(jumplist, jump_percent, "line1\nline2\nline3\nline4\nline5", "50%");

// NEW: More basic jump cases
neovim_test!(jumplist, jump_5G, "l1\nl2\nl3\nl4\nl5\nl6", "5G");
neovim_test!(jumplist, jump_1G, "line1\nline2\nline3", cursor(2, 0), "1G");
neovim_test!(jumplist, jump_gg_from_middle, "l1\nl2\nl3\nl4\nl5", cursor(2, 0), "gg");
neovim_test!(jumplist, jump_G_from_middle, "l1\nl2\nl3\nl4\nl5", cursor(2, 0), "G");
neovim_test!(jumplist, jump_25_percent, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8", "25%");
neovim_test!(jumplist, jump_75_percent, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8", "75%");

// ═══════════════════════════════════════════════════════════════════════════════
// CTRL-O (Jump to older position)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(jumplist, ctrl_o_after_G, "line1\nline2\nline3", "G<C-o>");
neovim_test!(jumplist, ctrl_o_after_gg, "line1\nline2\nline3", "Ggg<C-o>");
neovim_test!(jumplist, ctrl_o_multiple, "line1\nline2\nline3\nline4\nline5", "Gjj<C-o><C-o>");

// NEW: More Ctrl-O cases
neovim_test!(jumplist, ctrl_o_after_search, "hello\nworld\nhello", "/hello<CR><C-o>");
neovim_test!(jumplist, ctrl_o_after_mark, "line1\nline2\nline3", cursor(1, 0), "ma0'a<C-o>");
neovim_test!(jumplist, ctrl_o_chain, "l1\nl2\nl3\nl4\nl5", "GggGgg<C-o><C-o><C-o>");
neovim_test!(jumplist, ctrl_o_with_count, "line1\nline2\nline3\nline4", "GggG2<C-o>");
neovim_test!(jumplist, ctrl_o_at_start, "hello", "<C-o>");
neovim_test!(jumplist, ctrl_o_exceeds_list, "line1\nline2", "G<C-o><C-o><C-o>");

// ═══════════════════════════════════════════════════════════════════════════════
// CTRL-I (Jump to newer position)
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(jumplist, ctrl_i_after_ctrl_o, "line1\nline2\nline3", "G<C-o><C-i>");
neovim_test!(jumplist, ctrl_i_multiple, "line1\nline2\nline3\nline4", "Gjj<C-o><C-o><C-i>");

// NEW: More Ctrl-I cases
neovim_test!(jumplist, ctrl_i_full_cycle, "l1\nl2\nl3", "Ggg<C-o><C-i>");
neovim_test!(jumplist, ctrl_i_chain, "l1\nl2\nl3\nl4", "GggG<C-o><C-o><C-i><C-i>");
neovim_test!(jumplist, ctrl_i_with_count, "l1\nl2\nl3\nl4", "GggG<C-o><C-o>2<C-i>");
neovim_test!(jumplist, ctrl_i_at_end, "hello", "<C-i>");
neovim_test!(jumplist, ctrl_i_exceeds_list, "l1\nl2", "G<C-o><C-i><C-i><C-i>");
neovim_test!(jumplist, ctrl_o_ctrl_i_cycle, "l1\nl2\nl3", "G<C-o><C-i><C-o><C-i>");

// ═══════════════════════════════════════════════════════════════════════════════
// JUMP MOTIONS THAT PUSH TO JUMP LIST
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(jumplist, paragraph_forward_jumps, "line1\n\nline3", "}");
neovim_test!(jumplist, paragraph_backward_jumps, "line1\n\nline3", "G{");
neovim_test!(jumplist, sentence_forward_jumps, "Hello. World.", ")");

// NEW: More jump motion cases
neovim_test!(jumplist, paragraph_multiple, "a\n\nb\n\nc", "}}<C-o>");
neovim_test!(jumplist, paragraph_back_forward, "a\n\nb\n\nc", "G{{<C-o>");
neovim_test!(jumplist, sentence_back, "Hello. World.", "$(<C-o>");
neovim_test!(jumplist, bracket_match_jumps, "(hello)", "%<C-o>");
neovim_test!(jumplist, brace_match_jumps, "{hello}", "%<C-o>");

// ═══════════════════════════════════════════════════════════════════════════════
// MARKS PUSH TO JUMP LIST
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(jumplist, mark_jump_pushes, "line1\nline2\nline3", "jma0'a");
neovim_test!(jumplist, mark_exact_jump_pushes, "line1\nline2\nline3", "jlma0`a");

// NEW: More mark jump cases
neovim_test!(jumplist, mark_jump_then_back, "l1\nl2\nl3", cursor(1, 0), "ma0'a<C-o>");
neovim_test!(jumplist, mark_exact_then_back, "l1\nl2\nl3", cursor(1, 2), "ma0`a<C-o>");
neovim_test!(jumplist, multiple_marks_jump, "l1\nl2\nl3", "jmakmbj'a'b<C-o>");
neovim_test!(jumplist, global_mark_jump, "line1\nline2", cursor(1, 0), "mA0'A<C-o>");

// ═══════════════════════════════════════════════════════════════════════════════
// SEARCH PUSHES TO JUMP LIST
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(jumplist, search_forward_jumps, "foo\nbar\nfoo", "/foo<CR>");
neovim_test!(jumplist, search_next_jumps, "foo\nbar\nfoo", "/foo<CR>n");

// NEW: More search jump cases
neovim_test!(jumplist, search_backward_jumps, "foo\nbar\nfoo", cursor(2, 0), "?foo<CR><C-o>");
neovim_test!(jumplist, search_n_then_back, "foo bar foo", "/foo<CR>n<C-o>");
neovim_test!(jumplist, search_N_jumps, "foo bar foo", cursor(0, 8), "/foo<CR>N<C-o>");
neovim_test!(jumplist, star_search_jumps, "foo bar foo", "*<C-o>");
neovim_test!(jumplist, hash_search_jumps, "foo bar foo", cursor(0, 8), "#<C-o>");

// ═══════════════════════════════════════════════════════════════════════════════
// MOTIONS THAT DON'T PUSH TO JUMP LIST
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(jumplist, hjkl_no_push, "hello\nworld", "jjll<C-o>");
neovim_test!(jumplist, word_motion_no_push, "hello world foo", "www<C-o>");
neovim_test!(jumplist, find_no_push, "hello world", "fw<C-o>");
neovim_test!(jumplist, line_motion_no_push, "hello", "0$<C-o>");

// ═══════════════════════════════════════════════════════════════════════════════
// JUMP LIST WITH EDITS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(jumplist, jump_after_edit, "hello\nworld", "xG<C-o>");
neovim_test!(jumplist, jump_after_insert, "hello\nworld", "iX<Esc>G<C-o>");
neovim_test!(jumplist, edit_clears_forward, "l1\nl2\nl3", "G<C-o>xG<C-i>");

// ═══════════════════════════════════════════════════════════════════════════════
// EDGE CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(jumplist, jumplist_empty_buffer, "", "<C-o>");
neovim_test!(jumplist, single_line, "hello", "G<C-o>");
neovim_test!(jumplist, jumplist_single_char, "a", "G<C-o>");
neovim_test!(jumplist, unicode_jump, "日本語\nテスト", "G<C-o>");
neovim_test!(jumplist, long_document, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", "G50%gg<C-o><C-o>");
neovim_test!(jumplist, ctrl_o_i_alternating, "l1\nl2\nl3", "G<C-o><C-i><C-o><C-i>");
neovim_test!(jumplist, jump_to_same_line, "hello\nworld", "G0G<C-o>");

