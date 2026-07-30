// Scenario fidelity tests: Unicode Stress
//
// Edge cases that break Vim engines: multi-byte boundaries, combining characters,
// ZWJ sequences, RTL text, Devanagari conjuncts, surrogate-prone codepoints,
// mixed-script lines, and operations that must correctly handle variable-width
// grapheme clusters.
//
// NOTE: Some ZWJ-emoji tests are commented out because they can cause the
// Neovim oracle to hang (see basics.rs line 138).

// ═══════════════════════════════════════════════════════════════════════════════
// CJK DOUBLE-WIDTH CHARACTERS
// ═══════════════════════════════════════════════════════════════════════════════

// Each CJK character is its own word for w/b purposes.
neovim_test!(scenarios, uni_cjk_w_each_char_is_word, "中文测试", "w");
neovim_test!(scenarios, uni_cjk_b_back_across_chars, "中文测试", cursor(0, 3), "b");
neovim_test!(scenarios, uni_cjk_x_delete_first_char, "中文测试", "x");
neovim_test!(scenarios, uni_cjk_x_delete_last_char, "中文测试", cursor(0, 3), "x");
neovim_test!(scenarios, uni_cjk_cw_change_word, "中文 测试", "cwhello<Esc>");
neovim_test!(scenarios, uni_cjk_f_find_char, "中文测试文字", "f试");
neovim_test!(scenarios, uni_cjk_dw_on_mixed, "中文abc测试", "dw");
neovim_test!(scenarios, uni_cjk_dollar_end_of_line, "日本語テスト", "$");
neovim_test!(scenarios, uni_cjk_0_start_of_line, "日本語テスト", cursor(0, 5), "0");

// CJK in multiline context
neovim_test!(scenarios, uni_cjk_j_multiline, "日本語\n中文测试", "j");
neovim_test!(scenarios, uni_cjk_dd_whole_line, "日本語\n中文\n漢字", cursor(1, 0), "dd");
neovim_test!(scenarios, uni_cjk_yy_paste, "日本語\n中文", "yyp");

// ═══════════════════════════════════════════════════════════════════════════════
// EMOJI — SINGLE CODEPOINT AND MULTI-CODEPOINT
// ═══════════════════════════════════════════════════════════════════════════════

// Basic emoji (single codepoint, but multi-byte UTF-8)
neovim_test!(scenarios, uni_emoji_x_thumbsup, "👍test", "x");
neovim_test!(scenarios, uni_emoji_l_past_emoji, "👍test", "l");
neovim_test!(scenarios, uni_emoji_w_across_emoji, "👍 🎉 ✅", "w");
neovim_test!(scenarios, uni_emoji_dw_emoji_word, "👍 🎉 test", "dw");
neovim_test!(scenarios, uni_emoji_r_replace_emoji, "👍test", "rX");
neovim_test!(scenarios, uni_emoji_cw_emoji_to_ascii, "🎉 party", "cwdone<Esc>");

// Emoji with variation selector (U+FE0F)
neovim_test!(scenarios, uni_emoji_variation_sel_x, "✅️ done", "x");
neovim_test!(scenarios, uni_emoji_variation_sel_l, "❤️ love", "l");

// Flag emoji (regional indicator pairs)
neovim_test!(scenarios, uni_flag_emoji_x, "🇺🇸 USA", "x");
neovim_test!(scenarios, uni_flag_emoji_w, "🇺🇸 USA 🇬🇧 UK", "w");

// ZWJ sequences — commented out: may hang Neovim oracle
// neovim_test!(scenarios, uni_zwj_family_x, "👨‍👩‍👧‍👦 family", "x");
// neovim_test!(scenarios, uni_zwj_family_w, "👨‍👩‍👧‍👦 test", "w");
// neovim_test!(scenarios, uni_zwj_rainbow_flag_x, "🏳️‍🌈 pride", "x");

// ═══════════════════════════════════════════════════════════════════════════════
// COMBINING CHARACTERS
// ═══════════════════════════════════════════════════════════════════════════════

// e + combining acute accent (U+0301) — two codepoints, one grapheme
neovim_test!(scenarios, uni_combining_accent_l, "e\u{0301}cho", "l");
neovim_test!(scenarios, uni_combining_accent_x, "e\u{0301}cho", "x");
neovim_test!(scenarios, uni_combining_accent_w, "cafe\u{0301} latte", "w");
neovim_test!(scenarios, uni_combining_accent_f, "re\u{0301}sume\u{0301}", "fe\u{0301}");
neovim_test!(scenarios, uni_combining_accent_cw, "cafe\u{0301} latte", "cwtea<Esc>");

// n + combining tilde (U+0303)
neovim_test!(scenarios, uni_combining_tilde_x, "n\u{0303}ino", "x");

// Multiple combining marks stacked on one base
neovim_test!(scenarios, uni_multi_combining_x, "a\u{0300}\u{0301}bc", "x");
neovim_test!(scenarios, uni_multi_combining_l, "a\u{0300}\u{0301}bc", "l");

// ═══════════════════════════════════════════════════════════════════════════════
// RTL TEXT — ARABIC AND HEBREW
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, uni_arabic_w, "هذا نص عربي", "w");
neovim_test!(scenarios, uni_arabic_x, "مرحبا", "x");
neovim_test!(scenarios, uni_arabic_dw, "هذا نص", "dw");
neovim_test!(scenarios, uni_arabic_f, "هذا نص عربي", "fن");
neovim_test!(scenarios, uni_arabic_cw, "هذا نص", "cwhello<Esc>");

neovim_test!(scenarios, uni_hebrew_w, "שלום עולם", "w");
neovim_test!(scenarios, uni_hebrew_x, "שלום", "x");
neovim_test!(scenarios, uni_hebrew_dw, "שלום עולם", "dw");

// ═══════════════════════════════════════════════════════════════════════════════
// DEVANAGARI — CONJUNCTS AND VOWEL SIGNS
// ═══════════════════════════════════════════════════════════════════════════════

// Hindi word with consonant clusters and vowel signs
neovim_test!(scenarios, uni_devanagari_w, "हिन्दी भाषा", "w");
neovim_test!(scenarios, uni_devanagari_x, "हिन्दी", "x");
neovim_test!(scenarios, uni_devanagari_dw, "हिन्दी भाषा", "dw");
neovim_test!(scenarios, uni_devanagari_l, "नमस्ते", "l");

// ═══════════════════════════════════════════════════════════════════════════════
// MIXED SCRIPTS — THE HARDEST BOUNDARY CASES
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, uni_mixed_all_scripts_w, "Hello世界🌍مرحبا", "w");
neovim_test!(scenarios, uni_mixed_all_scripts_dw, "Hello世界🌍مرحبا", "dw");
neovim_test!(scenarios, uni_mixed_all_scripts_dollar, "Hello世界🌍مرحبا", "$");
neovim_test!(scenarios, uni_mixed_latin_cjk_w, "abc日本語def", "w");
neovim_test!(scenarios, uni_mixed_cjk_emoji_x, "日🎉本", "x");
neovim_test!(scenarios, uni_mixed_boundary_cw, "abc日本語 def", "cwhello<Esc>");

// Visual selection across script boundaries
neovim_test!(scenarios, uni_mixed_visual_select, "Hello世界test", "v4ld");
neovim_test!(scenarios, uni_mixed_visual_line, "Hello世界\nمرحبا\ntest", "Vjd");

// ═══════════════════════════════════════════════════════════════════════════════
// ZERO-WIDTH CHARACTERS
// ═══════════════════════════════════════════════════════════════════════════════

// Zero-width space (U+200B)
neovim_test!(scenarios, uni_zwsp_l, "a\u{200B}b\u{200B}c", "l");
neovim_test!(scenarios, uni_zwsp_x, "a\u{200B}b\u{200B}c", "x");
neovim_test!(scenarios, uni_zwsp_w, "hello\u{200B}world", "w");

// Zero-width non-joiner (U+200C) — used in Persian/Arabic
neovim_test!(scenarios, uni_zwnj_l, "a\u{200C}b", "l");
neovim_test!(scenarios, uni_zwnj_x, "a\u{200C}b", "x");

// Zero-width joiner (U+200D) — standalone, not in emoji context
neovim_test!(scenarios, uni_zwj_standalone_l, "a\u{200D}b", "l");

// ═══════════════════════════════════════════════════════════════════════════════
// SURROGATE-PRONE: CHARACTERS ABOVE U+FFFF (ASTRAL PLANE)
// ═══════════════════════════════════════════════════════════════════════════════

// Mathematical double-struck / fraktur (U+1D573–U+1D58A)
neovim_test!(scenarios, uni_astral_fraktur_l, "𝕳𝖊𝖑𝖑𝖔", "l");
neovim_test!(scenarios, uni_astral_fraktur_x, "𝕳𝖊𝖑𝖑𝖔", "x");
neovim_test!(scenarios, uni_astral_fraktur_w, "𝕳𝖊𝖑𝖑𝖔 world", "w");
neovim_test!(scenarios, uni_astral_fraktur_dw, "𝕳𝖊𝖑𝖑𝖔 world", "dw");

// Musical symbols (U+1D11E = 𝄞)
neovim_test!(scenarios, uni_astral_music_x, "𝄞𝄞𝄞", "x");
neovim_test!(scenarios, uni_astral_music_dollar, "𝄞𝄞𝄞", "$");

// Emoji above U+FFFF (U+1F600 = 😀)
neovim_test!(scenarios, uni_astral_emoji_l, "😀😁😂", "l");
neovim_test!(scenarios, uni_astral_emoji_x, "😀😁😂", "x");
neovim_test!(scenarios, uni_astral_emoji_dw, "😀 😁 😂", "dw");

// ═══════════════════════════════════════════════════════════════════════════════
// TAB + UNICODE MIXING
// ═══════════════════════════════════════════════════════════════════════════════

neovim_test!(scenarios, uni_tab_before_cjk, "\t日本語", "w");
neovim_test!(scenarios, uni_tab_after_cjk, "日本語\ttest", "w");
neovim_test!(scenarios, uni_tab_between_emoji, "👍\t🎉", "w");
neovim_test!(scenarios, uni_tab_mixed_dw, "\t日本語 test", "dw");

// ═══════════════════════════════════════════════════════════════════════════════
// OPERATIONS ON MULTI-BYTE BOUNDARIES
// ═══════════════════════════════════════════════════════════════════════════════

// Replace mode: overwrite emoji with ASCII
neovim_test!(scenarios, uni_replace_emoji_with_ascii, "👍test", "Ra<Esc>");
neovim_test!(scenarios, uni_replace_ascii_with_emoji, "atest", "R👍<Esc>");
neovim_test!(scenarios, uni_replace_cjk_with_ascii, "日本語", "Rabc<Esc>");
neovim_test!(scenarios, uni_replace_ascii_with_cjk, "abc", "R日本語<Esc>");

// Insert at multi-byte boundary
neovim_test!(scenarios, uni_insert_at_cjk_boundary, "日本語", cursor(0, 1), "iX<Esc>");
neovim_test!(scenarios, uni_append_after_emoji, "👍", "atest<Esc>");

// Delete on multi-byte boundary
neovim_test!(scenarios, uni_D_from_middle_of_cjk, "日本語テスト", cursor(0, 2), "D");
neovim_test!(scenarios, uni_C_from_middle_of_cjk, "日本語テスト", cursor(0, 2), "Chello<Esc>");

// Yank/paste Unicode text
neovim_test!(scenarios, uni_yank_paste_cjk, "日本語", "ywp");
neovim_test!(scenarios, uni_yank_paste_emoji, "👍 🎉", "ywp");
neovim_test!(scenarios, uni_yank_paste_mixed, "Hello世界", "yw$p");
neovim_test!(scenarios, uni_yank_line_paste_arabic, "مرحبا\nhello", "yyp");

// Search with Unicode patterns
neovim_test!(scenarios, uni_search_cjk, "hello 日本語 world", "/日本語<CR>");
neovim_test!(scenarios, uni_search_emoji, "test 🎉 here", "/🎉<CR>");
neovim_test!(scenarios, uni_search_arabic, "hello مرحبا world", "/مرحبا<CR>");
neovim_test!(scenarios, uni_search_accent, "cafe\u{0301} latte", "/cafe\u{0301}<CR>");

// Text objects with Unicode content
neovim_test!(scenarios, uni_diw_on_cjk, "hello 日本語 world", cursor(0, 6), "diw");
neovim_test!(scenarios, uni_ciw_on_emoji, "test 👍 done", cursor(0, 5), "ciwhello<Esc>");
neovim_test!(scenarios, uni_di_quote_cjk, "\"日本語\"", cursor(0, 1), "di\"");
neovim_test!(scenarios, uni_di_paren_mixed, "(Hello世界)", cursor(0, 1), "di(");
