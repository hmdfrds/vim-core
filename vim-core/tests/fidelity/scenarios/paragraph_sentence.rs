// Scenario fidelity tests: Paragraph and Sentence Motions
//
// Using }, {, ), ( as motions and operator targets in editing.

// ═══════════════════════════════════════════════════════════════════════════════
// PARAGRAPH MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, para_forward, "line1\nline2\n\nline3\nline4", "}");
neovim_test!(scenarios, para_backward, "line1\nline2\n\nline3\nline4", cursor(3, 0), "{");
neovim_test!(scenarios, para_fwd_twice, "a\n\nb\n\nc", "}}");
neovim_test!(scenarios, para_bwd_twice, "a\n\nb\n\nc", cursor(4, 0), "{{");
neovim_test!(scenarios, para_fwd_at_blank, "a\n\nb", cursor(1, 0), "}");
neovim_test!(scenarios, para_multiple_blanks, "a\n\n\n\nb", "}");

// ═══════════════════════════════════════════════════════════════════════════════
// PARAGRAPH AS OPERATOR TARGET
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, delete_paragraph, "delete\nme\n\nkeep", "d}");
neovim_test!(scenarios, yank_paragraph, "yank\nme\n\nrest", "y}Gp");
neovim_test!(scenarios, change_paragraph, "old\nstuff\n\nkeep", "c}new<Esc>");
neovim_test!(scenarios, indent_paragraph_op, "a\nb\nc\n\nd", ">}");
neovim_test!(scenarios, gU_paragraph, "lower\ncase\n\nrest", "gU}");
neovim_test!(scenarios, visual_paragraph, "a\nb\n\nc", "v}d");

// ═══════════════════════════════════════════════════════════════════════════════
// SENTENCE MOTIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, ps_sentence_forward, "First. Second. Third.", ")");
neovim_test!(scenarios, ps_sentence_backward, "First. Second. Third.", cursor(0, 15), "(");
neovim_test!(scenarios, sentence_fwd_twice, "One. Two. Three.", "))");
neovim_test!(scenarios, sentence_across_lines, "End here.\nNew sentence.", ")");

// ═══════════════════════════════════════════════════════════════════════════════
// SENTENCE AS OPERATOR TARGET
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, delete_sentence, "Delete me. Keep this.", "d)");
neovim_test!(scenarios, change_sentence, "Old sentence. Keep.", "c)New. <Esc>");
neovim_test!(scenarios, yank_sentence, "Copy me. Rest.", "y)$p");

// ═══════════════════════════════════════════════════════════════════════════════
// PARAGRAPH NAVIGATION THEN EDIT
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, jump_para_cw, "para1\n\ntarget word\n\npara3", "}wcwnew<Esc>");
neovim_test!(scenarios, jump_para_dd, "keep\n\ndelete\n\nkeep", "}jdd");
neovim_test!(scenarios, jump_para_O, "text\n\nmore text", "}Oinserted<Esc>");
