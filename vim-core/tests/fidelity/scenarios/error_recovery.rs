// Scenario fidelity tests: Error Recovery
//
// Multi-step workflows for recovering from mistakes.

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO ACCIDENTAL EDITS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, undo_word_delete, "important data", "dwu");
neovim_test!(scenarios, undo_line_delete, "keep this\naccident", cursor(1, 0), "ddu");
neovim_test!(scenarios, undo_bad_change_word, "correct word", "cwwrong<Esc>u");
neovim_test!(scenarios, undo_replace, "original", "rXu");
neovim_test!(scenarios, undo_paste, "clean", "ywpu");
neovim_test!(scenarios, undo_dd, "l1\nl2\nl3", cursor(1, 0), "ddu");

// ═══════════════════════════════════════════════════════════════════════════════
// MULTI-STEP UNDO
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, multi_undo_restore, "original", "cwfirst<Esc>cwsecond<Esc>uu");
neovim_test!(scenarios, undo_then_redo, "hello", "cwbye<Esc>u<C-r>");
neovim_test!(scenarios, undo_three_changes, "aaa", "rbrcrduuu");
neovim_test!(scenarios, undo_excess, "safe", "xu99u");

// ═══════════════════════════════════════════════════════════════════════════════
// ESCAPE FROM MODES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, escape_from_insert, "unchanged", "i<Esc>");
neovim_test!(scenarios, escape_from_visual, "unchanged", "v<Esc>");
neovim_test!(scenarios, escape_from_visual_line, "unchanged", "V<Esc>");
neovim_test!(scenarios, escape_command, "unchanged", ":<Esc>");
neovim_test!(scenarios, escape_search, "unchanged", "/partial<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO COMPLEX OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, undo_visual_word_delete, "hello world", "viwd u");
neovim_test!(scenarios, undo_visual_lines_delete, "l1\nl2\nl3", "Vjdu");
neovim_test!(scenarios, undo_join, "line one\nline two", "Ju");
neovim_test!(scenarios, undo_indent, "hello", "V>u");
neovim_test!(scenarios, undo_macro, "aaa\nbbb", "qaA!<Esc>jq@au");

// ═══════════════════════════════════════════════════════════════════════════════
// REDO PATTERNS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, redo_after_undo, "hello", "x u <C-r>");
neovim_test!(scenarios, redo_multiple, "hello", "xx uu <C-r><C-r>");
neovim_test!(scenarios, redo_change, "old", "cwnew<Esc>u<C-r>");
neovim_test!(scenarios, redo_dd, "l1\nl2\nl3", "ddu<C-r>");
neovim_test!(scenarios, redo_excess, "hello", "xu<C-r><C-r><C-r>");

// ═══════════════════════════════════════════════════════════════════════════════
// PARTIAL UNDO
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, undo_last_keep_first, "aaa", "ciwbbb<Esc>ciwccc<Esc>u");
neovim_test!(scenarios, undo_2_of_3, "aaa", "rbrcrduuu");
neovim_test!(scenarios, er_undo_redo_undo, "hello", "xu<C-r>xu");

// ═══════════════════════════════════════════════════════════════════════════════
// ESCAPE FROM WRONG MODE
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, er_escape_visual_block, "abc\nabc", "<C-v>j<Esc>");
neovim_test!(scenarios, escape_replace_mode, "hello", "R<Esc>");
neovim_test!(scenarios, er_double_escape, "hello", "i<Esc>v<Esc>:<Esc>");
neovim_test!(scenarios, escape_mid_insert, "hello world", "isome text<Esc>u");

// ═══════════════════════════════════════════════════════════════════════════════
// UNDO AFTER COMPLEX OPS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, er_undo_visual_indent, "a\nb", "Vj>u");
neovim_test!(scenarios, undo_visual_case, "hello", "VgUu");
neovim_test!(scenarios, undo_substitute, "old old old", ":s/old/new/g<CR>u");
neovim_test!(scenarios, er_undo_J, "l1\nl2", "Ju");
neovim_test!(scenarios, undo_gU, "hello world", "gUwu");

