// Scenario fidelity tests: Prose Editing
//
// Multi-step workflows for writing and editing prose/documentation.

// ═══════════════════════════════════════════════════════════════════════════════
// FIX SPELLING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, fix_spelling_cw, "teh quick brown", "cwthe<Esc>");
neovim_test!(scenarios, fix_spelling_r, "abd", cursor(0, 2), "rc");
neovim_test!(scenarios, fix_transpose, "wrod", cursor(0, 2), "xp");
neovim_test!(scenarios, fix_double_word, "the the fox", cursor(0, 4), "daw");
neovim_test!(scenarios, fix_missing_space, "helloworld", cursor(0, 5), "i <Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// SENTENCE / WORD OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, change_word_middle, "the quick brown fox", cursor(0, 4), "cwslow<Esc>");
neovim_test!(scenarios, prose_delete_word, "remove the extra word", cursor(0, 11), "daw");
neovim_test!(scenarios, insert_word, "the fox jumps", cursor(0, 4), "ibrown <Esc>");
neovim_test!(scenarios, capitalize_word, "hello world", "vUw");
neovim_test!(scenarios, lowercase_word, "HELLO world", "viwgu");

// ═══════════════════════════════════════════════════════════════════════════════
// MARKDOWN FORMATTING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, add_bold, "important", "i**<Esc>A**<Esc>");
neovim_test!(scenarios, add_italic, "emphasis", "i*<Esc>A*<Esc>");
neovim_test!(scenarios, add_heading, "Title", "I## <Esc>");
neovim_test!(scenarios, add_list_item, "item", "I- <Esc>");
neovim_test!(scenarios, add_code_inline, "code", "i`<Esc>A`<Esc>");
neovim_test!(scenarios, add_link_text, "click here", "I[<Esc>A](url)<Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// LINE EDITING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, join_sentences, "First sentence.\nSecond sentence.", "J");
neovim_test!(scenarios, split_line, "hello world test", cursor(0, 5), "i<CR><Esc>");
neovim_test!(scenarios, rewrite_line, "old content goes here", "ccnew content<Esc>");
neovim_test!(scenarios, append_to_line, "incomplete", "A sentence.<Esc>");
neovim_test!(scenarios, prepend_to_line, "content", "INote: <Esc>");

// ═══════════════════════════════════════════════════════════════════════════════
// SENTENCE REWRITING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, rewrite_sentence, "Old sentence here.", "ccNew sentence here.<Esc>");
neovim_test!(scenarios, delete_sentence_op, "Remove this. Keep this.", "d)");
neovim_test!(scenarios, change_sentence_op, "Old sentence. Keep.", "c)New one. <Esc>");
neovim_test!(scenarios, yank_paste_sentence, "Copy this. Target.", "y)$p");

// ═══════════════════════════════════════════════════════════════════════════════
// PARAGRAPH RESTRUCTURING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, delete_paragraph_op, "remove\nthis\n\nkeep", "d}");
neovim_test!(scenarios, join_paragraph, "line1\nline2\nline3", "JJ");
neovim_test!(scenarios, split_paragraph, "sentence one. sentence two.", "f.la<CR><Esc>");
neovim_test!(scenarios, move_sentence_down, "put last. put first.", "d)$p");

// ═══════════════════════════════════════════════════════════════════════════════
// LIST OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, add_bullet, "task", "I- <Esc>");
neovim_test!(scenarios, add_numbered, "task", "I1. <Esc>");
neovim_test!(scenarios, dup_list_item, "- item one", "yyp$bcwmod<Esc>");
neovim_test!(scenarios, delete_list_item, "- keep\n- remove\n- keep", cursor(1, 0), "dd");
neovim_test!(scenarios, indent_sub_item, "- parent\n- child", cursor(1, 0), "I  <Esc>");
neovim_test!(scenarios, swap_list_items, "- second\n- first", "ddp");

