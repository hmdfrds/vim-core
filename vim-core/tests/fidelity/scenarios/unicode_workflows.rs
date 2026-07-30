// Scenario fidelity tests: Unicode Workflows
//
// Real-world editing with CJK, emoji, accented characters.

// ═══════════════════════════════════════════════════════════════════════════════
// CJK EDITING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, cjk_delete_word, "日本語 テスト", "dw");
neovim_test!(scenarios, cjk_change_word, "日本語 テスト", "cwHello<Esc>");
neovim_test!(scenarios, cjk_yank_paste, "日本語 テスト", "yw$p");
neovim_test!(scenarios, cjk_append, "日本語", "A テスト<Esc>");
neovim_test!(scenarios, cjk_insert, "テスト", "I日本語 <Esc>");
neovim_test!(scenarios, cjk_delete_char, "日本語", "x");
neovim_test!(scenarios, cjk_replace_char, "日本語", "rX");
neovim_test!(scenarios, cjk_word_motion, "日本語 テスト 漢字", "ww");
neovim_test!(scenarios, cjk_back_motion, "日本語 テスト 漢字", cursor(0, 13), "bb");
neovim_test!(scenarios, cjk_find, "日本語テ日本", "f本");
neovim_test!(scenarios, cjk_inner_word, "日本語 テスト", "diw");

// ═══════════════════════════════════════════════════════════════════════════════
// EMOJI EDITING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, emoji_delete, "👍 test", "x");
neovim_test!(scenarios, emoji_append, "test 👍", "A 🎉<Esc>");
neovim_test!(scenarios, emoji_replace, "👍", "r🎉");
neovim_test!(scenarios, emoji_word_motion, "👍 🎉 ✅", "ww");
neovim_test!(scenarios, emoji_inner_word, "hello 👍 world", cursor(0, 6), "diw");
neovim_test!(scenarios, emoji_yank_paste, "👍 test", "yw$p");

// ═══════════════════════════════════════════════════════════════════════════════
// ACCENTED / EXTENDED LATIN
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, accent_word_motion, "café résumé naïve", "ww");
neovim_test!(scenarios, accent_change_word, "café latte", "cwtea<Esc>");
neovim_test!(scenarios, accent_find, "résumé", "fé");
neovim_test!(scenarios, accent_inner_word, "hello café world", cursor(0, 6), "diw");
neovim_test!(scenarios, accent_yank_paste, "über cool", "yw$p");

// ═══════════════════════════════════════════════════════════════════════════════
// MIXED SCRIPTS
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, mixed_cjk_latin, "Hello 世界 World", "wdw");
neovim_test!(scenarios, mixed_emoji_text, "Status: ✅ Done", cursor(0, 8), "daw");
neovim_test!(scenarios, mixed_accent_cjk, "café 日本語 über", "wwdw");
neovim_test!(scenarios, unicode_in_quotes, "\"日本語\"", cursor(0, 1), "di\"");
neovim_test!(scenarios, unicode_in_parens, "(テスト)", cursor(0, 1), "di(");
neovim_test!(scenarios, unicode_in_brackets, "[🎉 ✅]", cursor(0, 1), "di[");
