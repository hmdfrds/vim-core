// Scenario fidelity tests: Power User Tricks
//
// Advanced Vim commands that most users don't know about. Each test uses tricky
// initial text designed to expose edge cases in less-tested codepaths.

// =============================================================================
// gv - RESELECT LAST VISUAL (advanced edge cases)
// =============================================================================

// gv after a charwise yank then uppercase the same region
neovim_test!(scenarios, pw_gv_yank_then_gU, "mixedCase stuff", "veyygvgU");
// gv after visual-line delete: reselect should be sane on shorter buffer
neovim_test!(scenarios, pw_gv_after_Vd_on_shorter_buf, "aa\nbb\ncc\ndd", "Vjdgvd");

// =============================================================================
// g; / g, - CHANGELIST NAVIGATION (deeper edge cases)
// =============================================================================

// Navigate changelist across interleaved insertions on multiple lines
neovim_test!(scenarios, pw_changelist_interleaved, "alpha\nbeta\ngamma\ndelta", "AX<Esc>jjAY<Esc>jAZ<Esc>g;g;g;");
// g; with count: jump back 2 changes at once
neovim_test!(scenarios, pw_changelist_g_semicolon_count, "aa\nbb\ncc", "AX<Esc>jAY<Esc>jAZ<Esc>2g;");
// changelist after undo: the change positions should still be remembered
neovim_test!(scenarios, pw_changelist_after_undo, "one two three", "ciwA<Esc>wciwB<Esc>ug;");

// =============================================================================
// gi - GO TO LAST INSERT POSITION (tricky setups)
// =============================================================================

// gi after inserting in the middle of a word, then jumping far away
neovim_test!(scenarios, pw_gi_after_mid_word_insert, "abcdef\nghijkl\nmnopqr", cursor(0, 3), "iXYZ<Esc>GgiABC<Esc>");
// gi after an O (open above) on the last line
neovim_test!(scenarios, pw_gi_after_O_last_line, "first\nsecond\nthird", cursor(2, 0), "Onew<Esc>gggi!<Esc>");
// gi on an empty buffer after the first-ever insert
neovim_test!(scenarios, pw_gi_empty_buf_first_insert, "", "ihi<Esc>gi world<Esc>");

// =============================================================================
// "_d - BLACKHOLE REGISTER (preserving unnamed register)
// =============================================================================

// Yank a word, blackhole-delete another, paste the original yank
neovim_test!(scenarios, pw_blackhole_preserve_yank, "keep kill paste_here", "yiww\"_diw$p");
// Blackhole delete a line, verify unnamed register still has previous content
neovim_test!(scenarios, pw_blackhole_dd_then_put, "yanked_line\nkill_me\ntarget", "yyj\"_ddp");

// =============================================================================
// "+y / "*y - CLIPBOARD / PRIMARY SELECTION REGISTERS
// =============================================================================

// Yank a word to the + register then put it back
neovim_test!(scenarios, pw_clipboard_yank_word_put, "clipboard text here", "\"+yiw$\"+p");
// =============================================================================
// <C-a> / <C-x> - INCREMENT/DECREMENT (power-user edge cases)
// =============================================================================

// Increment a negative number to positive
neovim_test!(scenarios, pw_ctrl_a_neg_to_pos, "val = -1;", "f-<C-a>");
// Decrement across zero boundary with count
neovim_test!(scenarios, pw_ctrl_x_across_zero, "count: 2", "f23<C-x>");
// Increment with large count on a number embedded in text
neovim_test!(scenarios, pw_ctrl_a_embedded_number, "item_42_name", "f4100<C-a>");
// Dot repeat increment: increment, move to next line, dot
neovim_test!(scenarios, pw_ctrl_a_dot_next_line, "v1 = 10\nv2 = 20\nv3 = 30", "f1<C-a>j0.");
// Visual mode g<C-a> sequential increment on multiple lines
neovim_test!(scenarios, pw_visual_g_ctrl_a, "0\n0\n0\n0", "Vjjjg<C-a>");

// =============================================================================
// gU / gu - CASE OPERATORS WITH TRICKY MOTIONS
// =============================================================================

// gU with f motion: uppercase up to and including a punctuation char
neovim_test!(scenarios, pw_gU_f_dot, "hello.world.test", "gUf.");
// gu on visual block selection
neovim_test!(scenarios, pw_gu_visual_block, "ABC\nDEF\nGHI", "<C-v>2jlgu");
// g~ across a search motion
neovim_test!(scenarios, pw_g_tilde_search, "hello WORLD end", "g~/end<CR>");

// =============================================================================
// zz / zt / zb - SCROLL POSITIONING
// =============================================================================

// zz centers viewport; cursor stays
neovim_test!(scenarios, pw_zz_mid_buffer, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(5, 0), "zz");
// zt scrolls cursor line to top
neovim_test!(scenarios, pw_zt_mid_buffer, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(5, 0), "zt");
// zb scrolls cursor line to bottom
neovim_test!(scenarios, pw_zb_mid_buffer, "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\nl9\nl10", cursor(5, 0), "zb");
// zz then edit: make sure cursor col is preserved
neovim_test!(scenarios, pw_zz_then_edit, "hello world\nfoo bar\nbaz qux", cursor(1, 4), "zzx");

// =============================================================================
// <C-o> / <C-i> - JUMPLIST (complex navigation patterns)
// =============================================================================

// Jump via search, edit, jump back
neovim_test!(scenarios, pw_jumplist_search_edit_back, "start\nmiddle\ntarget\nend", "/target<CR>cwHIT<Esc><C-o>");
// Multiple jumps with marks: set marks, jump between, navigate jumplist
neovim_test!(scenarios, pw_jumplist_with_marks, "aaa\nbbb\nccc\nddd\neee", "majjmb''<C-o>");

// =============================================================================
// COMPLEX MACROS: qa...q then count @a
// =============================================================================

// Macro that wraps each line in quotes, replayed across lines
neovim_test!(scenarios, pw_macro_wrap_quotes, "alpha\nbeta\ngamma\ndelta", "qaI\"<Esc>A\"<Esc>jq3@a");
// Macro that deletes first word then appends to end, replayed
neovim_test!(scenarios, pw_macro_move_first_word, "rm_me keep1\nrm_me keep2\nrm_me keep3", "qadwA moved<Esc>jq2@a");
// Recursive macro: @a inside macro a (bounded by reaching end of buffer)
neovim_test!(scenarios, pw_macro_recursive_delete, "a\nb\nc\nd\ne\nf", "qaddq5@a");
// Macro with search inside
neovim_test!(scenarios, pw_macro_with_search, "TODO fix\nok line\nTODO fix\nok line", "qa/TODO<CR>ddq@a");

// =============================================================================
// g& - REPEAT LAST SUBSTITUTE GLOBALLY ON ALL LINES
// =============================================================================

// g& after a :s with no /g flag -- applies first match on every line
neovim_test!(scenarios, pw_g_ampersand_multi_line, "old new old\nold new old\nold new old", ":s/old/XXX<CR>g&");
// g& with regex substitution
neovim_test!(scenarios, pw_g_ampersand_regex, "foo123 bar\nfoo456 baz", ":s/foo\\d\\+/X<CR>g&");

// =============================================================================
// gn - SEARCH-AND-OPERATE PATTERN (the "cgn dot" refactor trick)
// =============================================================================

// cgn then dot repeat across the buffer
neovim_test!(scenarios, pw_cgn_dot_chain, "err err err ok err", "/err<CR>cgnfixed<Esc>...");
// dgn: delete next match, dot repeat
neovim_test!(scenarios, pw_dgn_dot_chain, "rm rm keep rm rm", "/rm<CR>dgn..");
// gn in visual mode to extend selection to next match
neovim_test!(scenarios, pw_gn_visual_extend, "ab cd ab ef ab", "/ab<CR>wvgn");

// =============================================================================
// <C-r>= - EXPRESSION REGISTER IN INSERT MODE
// =============================================================================

// Insert result of arithmetic
neovim_test!(scenarios, pw_expr_reg_arithmetic, "total = ", "A<C-r>=2+3<CR><Esc>");
// =============================================================================
// ]p / [p - PASTE WITH INDENT ADJUSTMENT
// =============================================================================

// Yank a shallow-indented line, paste-with-indent into a deeper scope
neovim_test!(scenarios, pw_bracket_p_deeper, "    outer\n        if true {\n            body\n        }", "yy2j]p");
// [p before a deeply indented line: should match current indent
neovim_test!(scenarios, pw_bracket_P_match_indent, "        deep\n    shallow", cursor(1, 0), "yy[p");

// =============================================================================
// COMBINING COUNTS: 3d2w = delete 6 words
// =============================================================================

// 2c3w: change 6 words
neovim_test!(scenarios, pw_count_2c3w, "a b c d e f g h i j", "2c3wX<Esc>");
// 3y2w then paste: yank 6 words
neovim_test!(scenarios, pw_count_3y2w_paste, "a b c d e f g h i j", "3y2w$p");
// Counts with gU: 2gU3w = uppercase 6 words
neovim_test!(scenarios, pw_count_2gU3w, "one two three four five six seven eight", "2gU3w");

// =============================================================================
// gJ - JOIN WITHOUT SPACE (edge cases)
// =============================================================================

// gJ preserves trailing whitespace (unlike J which collapses it)
neovim_test!(scenarios, pw_gJ_trailing_ws, "hello   \nworld", "gJ");
// gJ on line ending with period -- J would add space, gJ should not
neovim_test!(scenarios, pw_gJ_after_period, "end.\nStart", "gJ");
// gJ with count across multiple blank lines
neovim_test!(scenarios, pw_gJ_count_across_blanks, "a\n\n\nb", "4gJ");
// Visual gJ on three lines
neovim_test!(scenarios, pw_visual_gJ_three, "aa\nbb\ncc\ndd", "V2jgJ");

// =============================================================================
// g~ - SWAP CASE OPERATOR (edge cases)
// =============================================================================

// g~$ from middle of line: swap case to end
neovim_test!(scenarios, pw_g_tilde_to_eol, "heLLo WoRLd", cursor(0, 3), "g~$");
// g~~ (swap case entire line) on line with numbers and symbols
neovim_test!(scenarios, pw_g_tilde_tilde_mixed, "Hello_World_123!", "g~~");
// Visual-block g~ on a column
neovim_test!(scenarios, pw_g_tilde_block, "aBc\nDeF\ngHi", "<C-v>2jlg~");

// =============================================================================
// POWER-USER COMBOS - multi-trick sequences
// =============================================================================

// Star-search then cgn dot: the fastest refactor pattern in Vim
neovim_test!(scenarios, pw_star_cgn_dot, "old val old val old end", "*cgnNEW<Esc>..");
// Blackhole delete + gi: delete without clobbering, return to last insert
neovim_test!(scenarios, pw_blackhole_then_gi, "keep this\ndelete me\ninsert here", "ioops<Esc>j\"_ddgi fixed<Esc>");
// Macro that uses gn to replace all search matches
neovim_test!(scenarios, pw_macro_cgn_all, "bug bug ok bug", "/bug<CR>qacgnfix<Esc>q2@a");
