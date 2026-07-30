// Scenario fidelity tests: Deep Undo/Redo
//
// Comprehensive tests for Vim's undo tree model in complex, multi-step scenarios.
// Covers undo branches, redo invalidation, undo after macros, visual operations,
// substitute, insert-mode undo (C-u, C-w), U (undo line), counted undo/redo,
// and interactions between undo and other subsystems (registers, dot, macros).

// ═══════════════════════════════════════════════════════════════════════════════
// SIMPLE UNDO — baseline single-operation undo
// ═══════════════════════════════════════════════════════════════════════════════

// Insert text then undo the entire insert
neovim_test!(scenarios, deep_undo_simple_insert, "hello", "aXYZ<Esc>u");
// Delete a word with dw then undo
neovim_test!(scenarios, deep_undo_dw_restore, "alpha beta gamma", "dwu");
// Change word then undo restores original
neovim_test!(scenarios, deep_undo_cw_restore, "original text here", "cwreplaced<Esc>u");

// ═══════════════════════════════════════════════════════════════════════════════
// MULTI-STEP UNDO — sequential edits, undo one-by-one
// ═══════════════════════════════════════════════════════════════════════════════

// Three separate inserts (each enters+exits insert mode), undo all three
neovim_test!(scenarios, deep_undo_three_inserts, "base", "aA<Esc>aB<Esc>aC<Esc>uuu");
// Interleaved delete and insert, undo back to original
neovim_test!(scenarios, deep_undo_interleaved_ops, "foo bar baz", "dwcwQUX<Esc>xuu");
// Five x deletes, undo three of them (partial restore)
neovim_test!(scenarios, deep_undo_partial_restore_x, "abcdefgh", "xxxxxuuu");
// dd then cw then x, undo all
neovim_test!(scenarios, deep_undo_mixed_dd_cw_x, "line1\nline2\nline3", "ddjcwNEW<Esc>xuuu");
// Multiple inserts on different lines, undo each
neovim_test!(scenarios, deep_undo_multiline_inserts, "aaa\nbbb\nccc", "AX<Esc>jAY<Esc>jAZ<Esc>uuu");

// ═══════════════════════════════════════════════════════════════════════════════
// REDO AFTER PARTIAL UNDO — undo then redo forward
// ═══════════════════════════════════════════════════════════════════════════════

// Three edits, undo two, redo one
neovim_test!(scenarios, deep_redo_after_partial_undo, "start", "aA<Esc>aB<Esc>aC<Esc>uu<C-r>");
// Four deletes, undo all, redo two
neovim_test!(scenarios, deep_redo_two_of_four, "abcdefgh", "xxxxuuuu<C-r><C-r>");
// Undo all the way, then redo all the way back
neovim_test!(scenarios, deep_redo_full_roundtrip, "hello world", "dwxruuuu<C-r><C-r><C-r>");
// Undo/redo ping-pong: undo, redo, undo, redo
neovim_test!(scenarios, deep_redo_pingpong, "alpha", "xuu<C-r>u<C-r>");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO BRANCHES — edit after undo diverges the undo tree
// ═══════════════════════════════════════════════════════════════════════════════

// Edit, undo, new edit — redo of old edit is lost
neovim_test!(scenarios, deep_branch_new_edit_kills_redo, "original", "cwfirst<Esc>ucwsecond<Esc><C-r>");
// Two edits, undo both, new edit, try redo (should do nothing)
neovim_test!(scenarios, deep_branch_two_undo_new_edit, "base text", "dwxuuiNEW <Esc><C-r>");
// Branch after undo: insert after partial undo, then undo the branch
neovim_test!(scenarios, deep_branch_undo_the_branch, "ABCDE", "xxxxxuuuiZ<Esc>u");
// Edit, undo, edit, undo, edit — multiple branch points
neovim_test!(scenarios, deep_branch_multi_diverge, "test", "raurbucuuu");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO AFTER DIFFERENT OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

// Undo dd (delete line)
neovim_test!(scenarios, deep_undo_dd_line, "first\nsecond\nthird", cursor(1, 0), "ddu");
// Undo cw (change word)
neovim_test!(scenarios, deep_undo_cw_midline, "the quick brown fox", cursor(0, 4), "cwslow<Esc>u");
// Undo >> (indent)
neovim_test!(scenarios, deep_undo_indent_op, "no indent\n  some indent", ">>u");
// Undo << (outdent)
neovim_test!(scenarios, deep_undo_outdent_op, "    indented line", "<<u");
// Undo J (join lines)
neovim_test!(scenarios, deep_undo_join_two, "first line\nsecond line\nthird line", "Ju");
// Undo p (put after)
neovim_test!(scenarios, deep_undo_put_after, "hello world", "ywpu");
// Undo P (put before)
neovim_test!(scenarios, deep_undo_put_before, "hello world", "ywPu");
// Undo ~ (toggle case)
neovim_test!(scenarios, deep_undo_tilde, "hello", "~~~u");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO WITH COUNTS — 3u, 2<C-r>, etc.
// ═══════════════════════════════════════════════════════════════════════════════

// 3u undoes three changes at once
neovim_test!(scenarios, deep_undo_count_3, "abcdef", "raerberc3u");
// 2<C-r> redoes two changes at once
neovim_test!(scenarios, deep_redo_count_2, "abcdef", "rarbuuu2<C-r>");
// Counted undo exceeding history depth clamps to start
neovim_test!(scenarios, deep_undo_count_clamp, "abc", "xx50u");
// Counted redo exceeding redo stack clamps to end
neovim_test!(scenarios, deep_redo_count_clamp, "abcdef", "xxxuuu50<C-r>");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO VISUAL OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

// Visual character delete across words, then undo
neovim_test!(scenarios, deep_undo_vis_char_delete, "one two three four", "v2wdu");
// Visual line delete spanning 3 lines, then undo
neovim_test!(scenarios, deep_undo_vis_line_3, "alpha\nbeta\ngamma\ndelta", "Vjjdu");
// Visual block delete, then undo
neovim_test!(scenarios, deep_undo_vis_block_del, "abcd\nefgh\nijkl", "<C-v>jjldu");
// Visual indent two lines, then undo
neovim_test!(scenarios, deep_undo_vis_indent_2, "line1\nline2\nline3", "Vj>u");
// Visual gU (uppercase) then undo
neovim_test!(scenarios, deep_undo_vis_uppercase, "make me loud", "v$gUu");
// Visual gu (lowercase) then undo
neovim_test!(scenarios, deep_undo_vis_lowercase, "MAKE ME QUIET", "v$guu");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO SUBSTITUTE
// ═══════════════════════════════════════════════════════════════════════════════

// Simple :s then undo
neovim_test!(scenarios, deep_undo_sub_simple, "foo bar foo", ":s/foo/baz/<CR>u");
// Global :s/g then undo (one undo undoes entire substitute)
neovim_test!(scenarios, deep_undo_sub_global, "aaa bbb aaa bbb aaa", ":s/aaa/zzz/g<CR>u");
// Substitute then another edit, then undo both
neovim_test!(scenarios, deep_undo_sub_then_edit, "foo bar", ":s/foo/baz/<CR>xuu");

// ═══════════════════════════════════════════════════════════════════════════════
// INSERT-MODE UNDO — <C-u> and <C-w>
// ═══════════════════════════════════════════════════════════════════════════════

// <C-w> in insert mode deletes last word typed
neovim_test!(scenarios, deep_insert_ctrl_w, "base", "i hello world<C-w><Esc>");
// <C-u> in insert mode deletes to start of insert
neovim_test!(scenarios, deep_insert_ctrl_u, "base", "iHELLO WORLD<C-u><Esc>");
// <C-w> twice removes two words
neovim_test!(scenarios, deep_insert_ctrl_w_twice, "start", "i one two three<C-w><C-w><Esc>");
// <C-u> after typing on a blank line
neovim_test!(scenarios, deep_insert_ctrl_u_blank, "", "ihello there<C-u><Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// REDO COUNT — 2<C-r>, 3<C-r>
// ═══════════════════════════════════════════════════════════════════════════════

// 2<C-r> after 3 undos
neovim_test!(scenarios, deep_redo_count_after_3_undo, "abcdef", "rarbrcuuu2<C-r>");
// 3<C-r> after 5 undos
neovim_test!(scenarios, deep_redo_count_3_after_5_undo, "abcdefghij", "rarbrcrdre5u3<C-r>");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO JOIN (J)
// ═══════════════════════════════════════════════════════════════════════════════

// Join two lines, undo, verify both lines restored
neovim_test!(scenarios, deep_undo_J_two_lines, "hello\nworld", "Ju");
// Join three times, undo all three
neovim_test!(scenarios, deep_undo_J_three_times, "a\nb\nc\nd", "JJJuuu");
// Join with count, then undo
neovim_test!(scenarios, deep_undo_J_count, "aa\nbb\ncc\ndd", "3Ju");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO PASTE (p/P)
// ═══════════════════════════════════════════════════════════════════════════════

// Yank word, paste, undo — text back to before paste
neovim_test!(scenarios, deep_undo_paste_word, "hello world", "yiwwpu");
// Yank line, paste below, undo
neovim_test!(scenarios, deep_undo_paste_line, "only line", "yypu");
// Delete line (into register), paste elsewhere, undo paste
neovim_test!(scenarios, deep_undo_dd_then_paste, "first\nsecond\nthird", "ddjpu");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO INDENT/OUTDENT (>>, <<)
// ═══════════════════════════════════════════════════════════════════════════════

// >> then undo
neovim_test!(scenarios, deep_undo_indent_single, "no indent here", ">>u");
// >> twice then undo both
neovim_test!(scenarios, deep_undo_indent_double, "text", ">>>>uu");
// << on indented line then undo
neovim_test!(scenarios, deep_undo_outdent_single, "        deep indent", "<<u");
// Visual indent multiple lines then undo
neovim_test!(scenarios, deep_undo_vis_indent_multi, "a\nb\nc", "Vjj>u");

// ═══════════════════════════════════════════════════════════════════════════════
// COMPLEX MULTI-STEP: edit, undo back, redo forward
// ═══════════════════════════════════════════════════════════════════════════════

// Make 5 edits, undo 3, redo 1 — should be at edit 3
neovim_test!(scenarios, deep_complex_undo3_redo1, "start", "raerberc$ard$areuuuuu<C-r><C-r><C-r>");
// Edit different lines, undo back through all of them
neovim_test!(scenarios, deep_complex_multiline_undo, "aaa\nbbb\nccc", "AxA<Esc>jAxB<Esc>jAxC<Esc>uuu");
// Insert, delete, change, undo 2, redo 1, then new edit
neovim_test!(scenarios, deep_complex_edit_undo_redo_edit, "the cat sat", "ciwA<Esc>wdwwciwZ<Esc>uu<C-r>raX");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO AFTER MACRO — record macro, replay, then undo
// ═══════════════════════════════════════════════════════════════════════════════

// Record macro that deletes word, replay twice, undo each replay
neovim_test!(scenarios, deep_undo_after_macro_dw, "one two three four", "qadwq@a@auu");
// Record macro that appends text, replay, undo the replay
neovim_test!(scenarios, deep_undo_after_macro_append, "aa\nbb\ncc", "qaA!<Esc>jq2@auu");
// Record macro with cw, replay with count, undo
neovim_test!(scenarios, deep_undo_macro_cw_count, "old old old old", "qacwnew<Esc>wq2@auuu");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO LINE (U) — undo all changes on the current line
// ═══════════════════════════════════════════════════════════════════════════════

// U after single change
neovim_test!(scenarios, deep_U_single_change, "hello world", "raU");
// U after multiple changes on same line
neovim_test!(scenarios, deep_U_multi_change, "abcdef", "raxrfU");
// U on line after deleting characters
neovim_test!(scenarios, deep_U_after_deletes, "some text here", "xxdwU");
// U after insert on same line
neovim_test!(scenarios, deep_U_after_insert, "base", "iNEW <Esc>U");
// U does nothing if no changes on line
neovim_test!(scenarios, deep_U_no_changes, "untouched line", "U");
// U only affects current line — changes on other lines remain
neovim_test!(scenarios, deep_U_other_line_unaffected, "line1\nline2", "raXjraYkU");
// U after moving to the line and making changes
neovim_test!(scenarios, deep_U_after_move, "first\nsecond\nthird", "jcwSECOND<Esc>U");
// U is itself undoable with u
neovim_test!(scenarios, deep_U_then_u, "hello world", "dwUu");
