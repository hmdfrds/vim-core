use super::*;

// --- Line-number-based helpers ---

#[test]
fn test_line_start_by_number() {
    let text = "hello\nworld\nfoo";
    assert_eq!(line_start(text, 0), Some(0));
    assert_eq!(line_start(text, 1), Some(6));
    assert_eq!(line_start(text, 2), Some(12));
    assert_eq!(line_start(text, 3), None);
}

#[test]
fn test_line_end_by_number() {
    let text = "hello\nworld\nfoo";
    assert_eq!(line_end(text, 0), Some(5));
    assert_eq!(line_end(text, 1), Some(11));
    assert_eq!(line_end(text, 2), Some(15));
}

#[test]
fn test_line_of() {
    let text = "hello\nworld\nfoo";
    assert_eq!(line_of(text, 0), 0);
    assert_eq!(line_of(text, 5), 0);
    assert_eq!(line_of(text, 6), 1);
    assert_eq!(line_of(text, 12), 2);
}

#[test]
fn test_line_count() {
    // No trailing newline — \n is a separator
    assert_eq!(line_count("hello\nworld\nfoo"), 3);
    assert_eq!(line_count("single"), 1);
    assert_eq!(line_count(""), 0);
    // Trailing newline — \n is a separator, so "hello\n" = 2 lines
    // (matching Neovim's buffer model where trailing \n means empty last line)
    assert_eq!(line_count("hello\n"), 2);
    assert_eq!(line_count("hello\nworld\n"), 3);
    assert_eq!(line_count("hello\nworld\nfoo\n"), 4);
    // Only newlines — each \n is a separator
    assert_eq!(line_count("\n"), 2);
    assert_eq!(line_count("\n\n"), 3);
    assert_eq!(line_count("\n\n\n"), 4);
}

#[test]
fn test_line_content() {
    let text = "hello\nworld\nfoo";
    assert_eq!(line_content(text, 0), Some("hello"));
    assert_eq!(line_content(text, 1), Some("world"));
    assert_eq!(line_content(text, 2), Some("foo"));
}

#[test]
fn test_column_of() {
    let text = "hello\nworld";
    assert_eq!(column_of(text, 0), 0);
    assert_eq!(column_of(text, 3), 3);
    assert_eq!(column_of(text, 6), 0); // start of "world"
    assert_eq!(column_of(text, 8), 2);
}

// --- Offset-based helpers ---

#[test]
fn test_line_start_for_offset() {
    let text = "hello\nworld\nfoo";
    assert_eq!(line_start_for_offset(text, 0), 0);
    assert_eq!(line_start_for_offset(text, 3), 0);
    assert_eq!(line_start_for_offset(text, 6), 6);
    assert_eq!(line_start_for_offset(text, 8), 6);
}

#[test]
fn test_line_end_for_offset() {
    let text = "hello\nworld\nfoo";
    assert_eq!(line_end_for_offset(text, 0), 5);
    assert_eq!(line_end_for_offset(text, 6), 11);
    assert_eq!(line_end_for_offset(text, 12), 15);
}

#[test]
fn test_current_line() {
    let text = "hello\nworld\nfoo";
    assert_eq!(current_line(text, 0), "hello");
    assert_eq!(current_line(text, 6), "world");
    assert_eq!(current_line(text, 12), "foo");
}

// --- Non-blank helpers ---

#[test]
fn test_first_non_blank() {
    assert_eq!(first_non_blank_in_line("  hello"), 2);
    assert_eq!(first_non_blank_in_line("hello"), 0);
    assert_eq!(first_non_blank_in_line("   "), 2); // last char position
}

#[test]
fn test_last_non_blank() {
    assert_eq!(last_non_blank_in_line("hello  "), 4);
    assert_eq!(last_non_blank_in_line("hello"), 4);
}

// --- Grapheme movement ---

#[test]
fn test_move_left() {
    assert_eq!(move_left("hello", 3, 1), 2);
    assert_eq!(move_left("hello", 3, 5), 0);
    assert_eq!(move_left("hello", 0, 1), 0);
}

#[test]
fn test_move_right() {
    assert_eq!(move_right("hello", 0, 1), 1);
    assert_eq!(move_right("hello", 0, 10), 5);
}

// --- Character classification ---

#[test]
fn test_char_class_word() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('a', WordKind::Word, &wcs),
        CharClass::Word
    );
    assert_eq!(
        CharClass::classify('Z', WordKind::Word, &wcs),
        CharClass::Word
    );
    assert_eq!(
        CharClass::classify('5', WordKind::Word, &wcs),
        CharClass::Word
    );
    assert_eq!(
        CharClass::classify('_', WordKind::Word, &wcs),
        CharClass::Word
    );
    assert_eq!(
        CharClass::classify('.', WordKind::Word, &wcs),
        CharClass::Punctuation
    );
    assert_eq!(
        CharClass::classify('(', WordKind::Word, &wcs),
        CharClass::Punctuation
    );
    assert_eq!(
        CharClass::classify(' ', WordKind::Word, &wcs),
        CharClass::Whitespace
    );
    assert_eq!(
        CharClass::classify('\t', WordKind::Word, &wcs),
        CharClass::Whitespace
    );
}

#[test]
fn test_char_class_big_word() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('a', WordKind::WORD, &wcs),
        CharClass::Word
    );
    assert_eq!(
        CharClass::classify('.', WordKind::WORD, &wcs),
        CharClass::Word
    );
    assert_eq!(
        CharClass::classify('(', WordKind::WORD, &wcs),
        CharClass::Word
    );
    assert_eq!(
        CharClass::classify(' ', WordKind::WORD, &wcs),
        CharClass::Whitespace
    );
}

// --- Character boundaries ---

#[test]
fn test_char_boundaries() {
    let text = "héllo";
    assert_eq!(next_char_boundary(text, 0), 1);
    assert_eq!(next_char_boundary(text, 1), 3); // é is 2 bytes
    assert_eq!(prev_char_boundary(text, 3), 1);
}

// --- Blank line ---

#[test]
fn test_is_blank_line() {
    let text = "hello\n\n   \nworld";
    assert!(!is_blank_line(text, 0)); // "hello"
    assert!(is_blank_line(text, 6)); // ""
    assert!(is_blank_line(text, 7)); // "   "
    assert!(!is_blank_line(text, 11)); // "world"
}

// ═══════════════════════════════════════════════════════════════════════════════
// Virtual Column Helpers
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn byte_to_vcol_ascii_no_tabs() {
    // Pure ASCII, no tabs: byte offset == virtual column
    assert_eq!(byte_to_vcol("hello", 0, 8), 0);
    assert_eq!(byte_to_vcol("hello", 3, 8), 3);
    assert_eq!(byte_to_vcol("hello", 5, 8), 5);
}

#[test]
fn byte_to_vcol_with_tabs() {
    // Tab at start: '\t' expands to 8 columns (tabstop=8)
    // So 'h' at byte 1 is at vcol 8
    assert_eq!(byte_to_vcol("\thello", 0, 8), 0); // before tab
    assert_eq!(byte_to_vcol("\thello", 1, 8), 8); // after tab
    assert_eq!(byte_to_vcol("\thello", 2, 8), 9); // 'e'

    // Tab at col 2 with tabstop=4: expands to 2 spaces (4 - 2%4 = 2)
    // "ab\tcd" -> byte 0='a' vcol=0, byte 1='b' vcol=1, byte 2='\t' expands to vcol 4
    assert_eq!(byte_to_vcol("ab\tcd", 2, 4), 2); // before tab
    assert_eq!(byte_to_vcol("ab\tcd", 3, 4), 4); // after tab ('c')
    assert_eq!(byte_to_vcol("ab\tcd", 4, 4), 5); // 'd'
}

#[test]
fn byte_to_vcol_multiple_tabs() {
    // "\t\t" with tabstop=4: first tab=4 cols, second tab=4 cols
    assert_eq!(byte_to_vcol("\t\t", 0, 4), 0);
    assert_eq!(byte_to_vcol("\t\t", 1, 4), 4);
    assert_eq!(byte_to_vcol("\t\t", 2, 4), 8);
}

#[test]
fn byte_to_vcol_multibyte_chars() {
    // Multi-byte chars: each is 1 column but multiple bytes
    // "aé" = bytes [61, c3, a9] — 'é' is 2 bytes
    assert_eq!(byte_to_vcol("aé", 0, 4), 0); // before 'a'
    assert_eq!(byte_to_vcol("aé", 1, 4), 1); // after 'a', before 'é'
    assert_eq!(byte_to_vcol("aé", 3, 4), 2); // after 'é'
}

#[test]
fn byte_to_vcol_clamped_to_line_len() {
    assert_eq!(byte_to_vcol("hi", 100, 4), 2);
}

#[test]
fn byte_to_vcol_empty_line() {
    assert_eq!(byte_to_vcol("", 0, 4), 0);
}

#[test]
fn byte_to_vcol_tabstop_zero_clamped() {
    // tabstop=0 should be clamped to 1
    assert_eq!(byte_to_vcol("\tx", 1, 0), 1);
}

#[test]
fn vcol_to_byte_ascii_no_tabs() {
    assert_eq!(vcol_to_byte("hello", 0, 8), 0);
    assert_eq!(vcol_to_byte("hello", 3, 8), 3);
    assert_eq!(vcol_to_byte("hello", 5, 8), 5);
}

#[test]
fn vcol_to_byte_with_tabs() {
    // "\thello" with tabstop=8: vcol 8 -> byte 1
    assert_eq!(vcol_to_byte("\thello", 8, 8), 1);
    assert_eq!(vcol_to_byte("\thello", 9, 8), 2);

    // "ab\tcd" with tabstop=4: vcol 4 -> byte 3 ('c')
    assert_eq!(vcol_to_byte("ab\tcd", 4, 4), 3);
    assert_eq!(vcol_to_byte("ab\tcd", 5, 4), 4);
}

#[test]
fn vcol_to_byte_beyond_line() {
    // vcol beyond line length -> line length
    assert_eq!(vcol_to_byte("hi", 100, 4), 2);
}

#[test]
fn vcol_to_byte_multibyte() {
    // "aé" — vcol 2 means after 'é' which is at byte 3
    assert_eq!(vcol_to_byte("aé", 2, 4), 3);
}

#[test]
fn vcol_to_byte_roundtrip() {
    // byte -> vcol -> byte should be identity for non-tab non-multibyte text
    let line = "hello world";
    for i in 0..=line.len() {
        let vcol = byte_to_vcol(line, i, 4);
        let back = vcol_to_byte(line, vcol, 4);
        assert_eq!(back, i, "roundtrip failed at byte {i}");
    }
}

#[test]
fn vcol_to_byte_roundtrip_with_tabs() {
    // For tab-containing text, byte->vcol->byte should land on the same position
    // only when the byte is at a char boundary after a tab
    let line = "\thello";
    // byte 1 (after tab) -> vcol 8 -> byte 1
    let vcol = byte_to_vcol(line, 1, 8);
    assert_eq!(vcol, 8);
    assert_eq!(vcol_to_byte(line, 8, 8), 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Emoji CharClass
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn classify_emoji_thumbs_up() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('👍', WordKind::Word, &wcs),
        CharClass::Emoji
    );
}

#[test]
fn classify_emoji_party() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('🎉', WordKind::Word, &wcs),
        CharClass::Emoji
    );
}

#[test]
fn classify_emoji_grinning_face() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('😀', WordKind::Word, &wcs),
        CharClass::Emoji
    );
}

#[test]
fn classify_emoji_rocket() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('🚀', WordKind::Word, &wcs),
        CharClass::Emoji
    );
}

#[test]
fn classify_emoji_regional_indicator() {
    // Regional Indicators have the Alphabetic property — emoji check must win.
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{1F1E6}', WordKind::Word, &wcs), // 🇦
        CharClass::Emoji
    );
}

#[test]
fn classify_emoji_not_whitespace() {
    // Emoji must not be classified as whitespace
    assert!(!CharClass::Emoji.is_whitespace());
}

#[test]
fn classify_emoji_word_boundary() {
    // Word motion should stop at emoji boundary: "hello👍world"
    // 'o' = Word, '👍' = Emoji — different classes
    let wcs = WordCharSet::default_vim();
    let class_o = CharClass::classify('o', WordKind::Word, &wcs);
    let class_emoji = CharClass::classify('👍', WordKind::Word, &wcs);
    assert_ne!(class_o, class_emoji);
}

#[test]
fn classify_emoji_dingbat_check() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('✔', WordKind::Word, &wcs), // U+2714
        CharClass::Emoji
    );
}

#[test]
fn classify_emoji_misc_symbol() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('☀', WordKind::Word, &wcs), // U+2600
        CharClass::Emoji
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Non-Latin Punctuation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn classify_greek_question_mark() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{037E}', WordKind::Word, &wcs), // Greek question mark
        CharClass::Punctuation
    );
}

#[test]
fn classify_greek_ano_teleia() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{0387}', WordKind::Word, &wcs), // Greek ano teleia
        CharClass::Punctuation
    );
}

#[test]
fn classify_arabic_comma() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{060C}', WordKind::Word, &wcs), // Arabic comma
        CharClass::Punctuation
    );
}

#[test]
fn classify_arabic_question_mark() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{061F}', WordKind::Word, &wcs), // Arabic question mark
        CharClass::Punctuation
    );
}

#[test]
fn classify_devanagari_danda() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{0964}', WordKind::Word, &wcs), // Devanagari danda
        CharClass::Punctuation
    );
}

#[test]
fn classify_armenian_apostrophe() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{055A}', WordKind::Word, &wcs), // Armenian apostrophe
        CharClass::Punctuation
    );
}

#[test]
fn classify_hebrew_maqaf() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{05BE}', WordKind::Word, &wcs), // Hebrew maqaf
        CharClass::Punctuation
    );
}

#[test]
fn classify_thai_paiyannoi() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{0E2F}', WordKind::Word, &wcs), // Thai paiyannoi
        CharClass::Punctuation
    );
}

#[test]
fn classify_myanmar_sign() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{104A}', WordKind::Word, &wcs), // Myanmar sign
        CharClass::Punctuation
    );
}

#[test]
fn classify_ethiopic_full_stop() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{1362}', WordKind::Word, &wcs), // Ethiopic full stop
        CharClass::Punctuation
    );
}

#[test]
fn classify_mongolian_ellipsis() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{1801}', WordKind::Word, &wcs), // Mongolian ellipsis
        CharClass::Punctuation
    );
}

#[test]
fn classify_khmer_sign() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{17D4}', WordKind::Word, &wcs), // Khmer sign
        CharClass::Punctuation
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// CJK Compatibility (0x3300-0x33FF)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn classify_cjk_compatibility_3300() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{3300}', WordKind::Word, &wcs),
        CharClass::CjkIdeograph
    );
}

#[test]
fn classify_cjk_compatibility_33ff() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{33FF}', WordKind::Word, &wcs),
        CharClass::CjkIdeograph
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Superscript / Subscript / Braille
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn classify_superscript_2() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{00B2}', WordKind::Word, &wcs), // ²
        CharClass::Punctuation
    );
}

#[test]
fn classify_superscript_3() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{00B3}', WordKind::Word, &wcs), // ³
        CharClass::Punctuation
    );
}

#[test]
fn classify_superscript_1() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{00B9}', WordKind::Word, &wcs), // ¹
        CharClass::Punctuation
    );
}

#[test]
fn classify_superscript_block() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{2070}', WordKind::Word, &wcs), // ⁰
        CharClass::Punctuation
    );
}

#[test]
fn classify_subscript() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{2080}', WordKind::Word, &wcs), // ₀
        CharClass::Punctuation
    );
}

#[test]
fn classify_braille() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{2800}', WordKind::Word, &wcs), // Braille blank
        CharClass::Punctuation
    );
    assert_eq!(
        CharClass::classify('\u{28FF}', WordKind::Word, &wcs), // Braille end
        CharClass::Punctuation
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Unicode Symbol Ranges
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn classify_math_bold_a() {
    let wcs = WordCharSet::default_vim();
    // U+1D400 Mathematical Bold Capital A — is_alphabetic() returns true,
    // but must be classified as Punctuation.
    assert_eq!(
        CharClass::classify('\u{1D400}', WordKind::Word, &wcs),
        CharClass::Punctuation
    );
}

#[test]
fn classify_math_italic() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{1D434}', WordKind::Word, &wcs), // 𝐴 Math italic A
        CharClass::Punctuation
    );
}

#[test]
fn classify_currency_euro() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{20AC}', WordKind::Word, &wcs), // €
        CharClass::Punctuation
    );
}

#[test]
fn classify_arrow_right() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{2192}', WordKind::Word, &wcs), // →
        CharClass::Punctuation
    );
}

#[test]
fn classify_math_operator() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('\u{2200}', WordKind::Word, &wcs), // ∀
        CharClass::Punctuation
    );
    assert_eq!(
        CharClass::classify('\u{2211}', WordKind::Word, &wcs), // ∑
        CharClass::Punctuation
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Regression: existing classifications still work
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn classify_existing_cjk_still_works() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('日', WordKind::Word, &wcs),
        CharClass::CjkIdeograph
    );
    assert_eq!(
        CharClass::classify('テ', WordKind::Word, &wcs),
        CharClass::Katakana
    );
    assert_eq!(
        CharClass::classify('あ', WordKind::Word, &wcs),
        CharClass::Hiragana
    );
    assert_eq!(
        CharClass::classify('한', WordKind::Word, &wcs),
        CharClass::Hangul
    );
}

#[test]
fn classify_existing_ascii_still_works() {
    let wcs = WordCharSet::default_vim();
    assert_eq!(
        CharClass::classify('a', WordKind::Word, &wcs),
        CharClass::Word
    );
    assert_eq!(
        CharClass::classify('.', WordKind::Word, &wcs),
        CharClass::Punctuation
    );
    assert_eq!(
        CharClass::classify(' ', WordKind::Word, &wcs),
        CharClass::Whitespace
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// fold_snap() tests
// ═══════════════════════════════════════════════════════════════════════════════

mod fold_snap_tests {
    use super::*;
    use crate::document::FoldProvider;
    use crate::primitives::{Direction, LineNumber};

    /// Test fold provider: lines 2-4 are folded (0-indexed).
    struct FoldLines2To4;
    impl FoldProvider for FoldLines2To4 {
        fn next_visible_line(&self, line: LineNumber, dir: Direction) -> LineNumber {
            if (2..=4).contains(&line.get()) {
                match dir {
                    Direction::Forward => LineNumber::new(5),
                    Direction::Backward => LineNumber::new(1),
                }
            } else {
                line
            }
        }
        fn is_folded(&self, line: LineNumber) -> bool {
            (2..=4).contains(&line.get())
        }
    }

    fn text_6_lines() -> &'static str {
        // line0\nline1\nline2\nline3\nline4\nline5
        // 0     6      12     18     24     30
        "line0\nline1\nline2\nline3\nline4\nline5"
    }

    #[test]
    fn outside_fold_returns_pos_unchanged() {
        let fold = FoldLines2To4;
        let text = text_6_lines();
        // line 0, offset 2 — not folded
        assert_eq!(fold_snap(text, 2, Direction::Forward, &fold), 2);
        assert_eq!(fold_snap(text, 2, Direction::Backward, &fold), 2);
        // line 5, offset 32
        assert_eq!(fold_snap(text, 32, Direction::Forward, &fold), 32);
        assert_eq!(fold_snap(text, 32, Direction::Backward, &fold), 32);
    }

    #[test]
    fn inside_fold_forward_snaps_to_fold_end() {
        let fold = FoldLines2To4;
        let text = text_6_lines();
        // line 3 (offset 18) is inside fold lines 2-4.
        // Fold end = line 4, line_end(text, 4) = 29 (offset of 'e' in "line4")
        let result = fold_snap(text, 18, Direction::Forward, &fold);
        assert_eq!(result, line_end(text, 4).unwrap());
    }

    #[test]
    fn inside_fold_backward_snaps_to_fold_start() {
        let fold = FoldLines2To4;
        let text = text_6_lines();
        // line 3 (offset 18) is inside fold lines 2-4.
        // Fold start = line 2, line_start(text, 2) = 12
        let result = fold_snap(text, 18, Direction::Backward, &fold);
        assert_eq!(result, 12);
    }

    #[test]
    fn fold_start_line_snaps_correctly() {
        let fold = FoldLines2To4;
        let text = text_6_lines();
        // line 2 (offset 12) — first folded line
        assert_eq!(fold_snap(text, 12, Direction::Backward, &fold), 12);
        assert_eq!(
            fold_snap(text, 12, Direction::Forward, &fold),
            line_end(text, 4).unwrap()
        );
    }

    #[test]
    fn fold_end_line_snaps_correctly() {
        let fold = FoldLines2To4;
        let text = text_6_lines();
        // line 4 (offset 24) — last folded line
        assert_eq!(fold_snap(text, 24, Direction::Backward, &fold), 12);
        assert_eq!(
            fold_snap(text, 24, Direction::Forward, &fold),
            line_end(text, 4).unwrap()
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Quality audit fixes: ZWSP + Unicode range boundary tests
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn classify_zwsp_as_whitespace() {
    let wc = WordCharSet::default();
    assert_eq!(
        CharClass::classify('\u{200B}', WordKind::Word, &wc),
        CharClass::Whitespace,
        "U+200B ZWSP must be whitespace"
    );
}

#[test]
fn classify_halfwidth_hangul_jamo() {
    let wc = WordCharSet::default();
    assert_eq!(
        CharClass::classify('\u{FFA0}', WordKind::Word, &wc),
        CharClass::Hangul,
        "U+FFA0 (halfwidth filler) must be Hangul"
    );
    assert_eq!(
        CharClass::classify('\u{FFDC}', WordKind::Word, &wc),
        CharClass::Hangul,
        "U+FFDC (halfwidth jamo end) must be Hangul"
    );
}

#[test]
fn classify_unicode_range_boundaries() {
    let wc = WordCharSet::default();

    // Hiragana boundaries
    assert_eq!(
        CharClass::classify('\u{3041}', WordKind::Word, &wc),
        CharClass::Hiragana
    );
    assert_eq!(
        CharClass::classify('\u{3096}', WordKind::Word, &wc),
        CharClass::Hiragana
    );

    // Katakana boundaries
    assert_eq!(
        CharClass::classify('\u{30A1}', WordKind::Word, &wc),
        CharClass::Katakana
    );
    assert_eq!(
        CharClass::classify('\u{30FA}', WordKind::Word, &wc),
        CharClass::Katakana
    );

    // CJK Unified Ideographs boundaries
    assert_eq!(
        CharClass::classify('\u{4E00}', WordKind::Word, &wc),
        CharClass::CjkIdeograph
    );
    assert_eq!(
        CharClass::classify('\u{9FFF}', WordKind::Word, &wc),
        CharClass::CjkIdeograph
    );

    // CJK Compatibility (3300-33FF)
    assert_eq!(
        CharClass::classify('\u{3300}', WordKind::Word, &wc),
        CharClass::CjkIdeograph
    );
    assert_eq!(
        CharClass::classify('\u{33FF}', WordKind::Word, &wc),
        CharClass::CjkIdeograph
    );

    // Hangul Syllables boundaries
    assert_eq!(
        CharClass::classify('\u{AC00}', WordKind::Word, &wc),
        CharClass::Hangul
    );
    assert_eq!(
        CharClass::classify('\u{D7AF}', WordKind::Word, &wc),
        CharClass::Hangul
    );

    // Math alphanumeric (must be Punctuation, not Word via is_alphabetic)
    assert_eq!(
        CharClass::classify('\u{1D400}', WordKind::Word, &wc),
        CharClass::Punctuation
    );
    assert_eq!(
        CharClass::classify('\u{1D7FF}', WordKind::Word, &wc),
        CharClass::Punctuation
    );

    // Braille boundaries
    assert_eq!(
        CharClass::classify('\u{2800}', WordKind::Word, &wc),
        CharClass::Punctuation
    );
    assert_eq!(
        CharClass::classify('\u{28FF}', WordKind::Word, &wc),
        CharClass::Punctuation
    );

    // Superscript/subscript block
    assert_eq!(
        CharClass::classify('\u{2070}', WordKind::Word, &wc),
        CharClass::Punctuation
    );
    assert_eq!(
        CharClass::classify('\u{209F}', WordKind::Word, &wc),
        CharClass::Punctuation
    );
}

// ── compute_block_geometry: tab-aware virtual column tests ───────────

#[test]
fn test_block_geometry_plain_ascii() {
    // "hello\nworld" — anchor at 'h' (0), head at 'w' (6)
    // Both at grapheme col 0 and vcol 0, no tabs.
    let text = "hello\nworld";
    let geo = compute_block_geometry(text, Offset::new(0), Offset::new(6), 8);
    assert_eq!(geo.top_line, 0);
    assert_eq!(geo.bot_line, 1);
    assert_eq!(geo.left_gcol, 0);
    assert_eq!(geo.right_gcol, 0);
    assert_eq!(geo.left_vcol, 0);
    assert_eq!(geo.right_vcol, 0);
}

#[test]
fn test_block_geometry_plain_ascii_midline() {
    // "hello\nworld" — anchor at 'l' (2), head at 'r' (8)
    // Both at grapheme col 2, vcol 2.
    let text = "hello\nworld";
    let geo = compute_block_geometry(text, Offset::new(2), Offset::new(8), 8);
    assert_eq!(geo.left_gcol, 2);
    assert_eq!(geo.right_gcol, 2);
    assert_eq!(geo.left_vcol, 2);
    assert_eq!(geo.right_vcol, 2);
}

#[test]
fn test_block_geometry_with_tabs() {
    // Line 0: "\thello" — tab at col 0, 'h' at byte 1
    // Line 1: "\tworld" — tab at col 0, 'w' at byte 7
    // Anchor at byte 1 ('h'), head at byte 7 ('w')
    // Grapheme col for both: 1 (tab is 1 grapheme)
    // Virtual col for both: 8 (tab expands to 8 with tabstop=8)
    let text = "\thello\n\tworld";
    let geo = compute_block_geometry(text, Offset::new(1), Offset::new(8), 8);
    assert_eq!(geo.top_line, 0);
    assert_eq!(geo.bot_line, 1);
    assert_eq!(geo.left_gcol, 1);
    assert_eq!(geo.right_gcol, 1);
    assert_eq!(geo.left_vcol, 8);
    assert_eq!(geo.right_vcol, 8);
}

#[test]
fn test_block_geometry_tab_vs_spaces() {
    // Line 0: "\tx" — tab + 'x': 'x' is at gcol 1, vcol 8 (tabstop=8)
    //   bytes: \t=0, x=1, \n=2
    // Line 1: "        x" — 8 spaces + 'x': 'x' is at gcol 8, vcol 8
    //   bytes: line starts at 3, spaces=3..11, x=11
    // Anchor at 'x' on line 0 (byte 1), head at 'x' on line 1 (byte 11)
    // gcol: min(1,8)=1, max(1,8)=8 — wide grapheme column range!
    // vcol: both 8 — narrow, correct block alignment
    let text = "\tx\n        x";
    let geo = compute_block_geometry(text, Offset::new(1), Offset::new(11), 8);
    assert_eq!(geo.left_gcol, 1);
    assert_eq!(geo.right_gcol, 8);
    // Virtual columns should be equal since both 'x' are at screen col 8
    assert_eq!(geo.left_vcol, 8);
    assert_eq!(geo.right_vcol, 8);
}

#[test]
fn test_block_geometry_different_tabstop() {
    // "\thello" with tabstop=4: tab expands to 4 cols
    // Anchor at byte 1 ('h'), head at byte 1+7=8 ('w' on line 1)
    let text = "\thello\n\tworld";
    let geo = compute_block_geometry(text, Offset::new(1), Offset::new(8), 4);
    assert_eq!(geo.left_vcol, 4);
    assert_eq!(geo.right_vcol, 4);
}

#[test]
fn test_block_geometry_mixed_tabs_different_positions() {
    // Line 0: "ab\tcd" — 'c' at byte 3, gcol 3, vcol 8 (tab at vcol 2 expands to 8)
    // Line 1: "xy\tuv" — 'u' at byte 3+6=9, gcol 3, vcol 8
    let text = "ab\tcd\nxy\tuv";
    let geo_anchor = Offset::new(3); // 'c' on line 0
    let geo_head = Offset::new(9); // 'u' on line 1
    let geo = compute_block_geometry(text, geo_anchor, geo_head, 8);
    assert_eq!(geo.left_gcol, 3);
    assert_eq!(geo.right_gcol, 3);
    assert_eq!(geo.left_vcol, 8);
    assert_eq!(geo.right_vcol, 8);
}

// ═══════════════════════════════════════════════════════════════════════════════
// split_search_chain tests
// ═══════════════════════════════════════════════════════════════════════════════

use super::split_search_chain;
use crate::primitives::Direction;

#[test]
fn split_search_chain_no_chain() {
    let segs = split_search_chain("foo");
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].input, "foo");
    assert_eq!(segs[0].direction_override, None);
}

#[test]
fn split_search_chain_with_offset_no_chain() {
    // "/foo/e+3" — offset but no chain
    let segs = split_search_chain("foo/e+3");
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].input, "foo/e+3");
    assert_eq!(segs[0].direction_override, None);
}

#[test]
fn split_search_chain_forward_then_backward() {
    // "/foo/;?bar" → input is "foo/;?bar"
    let segs = split_search_chain("foo/;?bar");
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].input, "foo/");
    assert_eq!(segs[0].direction_override, None);
    assert_eq!(segs[1].input, "bar");
    assert_eq!(segs[1].direction_override, Some(Direction::Backward));
}

#[test]
fn split_search_chain_forward_then_forward() {
    // "/foo/;/bar" → input is "foo/;/bar"
    let segs = split_search_chain("foo/;/bar");
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].input, "foo/");
    assert_eq!(segs[0].direction_override, None);
    assert_eq!(segs[1].input, "bar");
    assert_eq!(segs[1].direction_override, Some(Direction::Forward));
}

#[test]
fn split_search_chain_three_segments() {
    // "/foo/;/bar/+3;?baz" → "foo/;/bar/+3;?baz"
    let segs = split_search_chain("foo/;/bar/+3;?baz");
    assert_eq!(segs.len(), 3);
    assert_eq!(segs[0].input, "foo/");
    assert_eq!(segs[0].direction_override, None);
    assert_eq!(segs[1].input, "bar/+3");
    assert_eq!(segs[1].direction_override, Some(Direction::Forward));
    assert_eq!(segs[2].input, "baz");
    assert_eq!(segs[2].direction_override, Some(Direction::Backward));
}

#[test]
fn split_search_chain_escaped_semicolon() {
    // Pattern contains literal \; — should NOT split
    let segs = split_search_chain("foo\\;/bar");
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].input, "foo\\;/bar");
}

#[test]
fn split_search_chain_semicolon_without_slash() {
    // "foo;bar" — semicolon not followed by / or ? → not a chain
    let segs = split_search_chain("foo;bar");
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].input, "foo;bar");
}

#[test]
fn split_search_chain_no_trailing_delimiter() {
    // "/foo;?bar" → input is "foo;?bar" (no trailing / on first pattern)
    let segs = split_search_chain("foo;?bar");
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].input, "foo");
    assert_eq!(segs[0].direction_override, None);
    assert_eq!(segs[1].input, "bar");
    assert_eq!(segs[1].direction_override, Some(Direction::Backward));
}

#[test]
fn split_search_chain_empty_first_segment() {
    // "/;?bar" → input is ";?bar" — empty first pattern (reuse last pattern)
    let segs = split_search_chain(";?bar");
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].input, "");
    assert_eq!(segs[0].direction_override, None);
    assert_eq!(segs[1].input, "bar");
    assert_eq!(segs[1].direction_override, Some(Direction::Backward));
}
