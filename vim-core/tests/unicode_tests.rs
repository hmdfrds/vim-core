//! Unicode and grapheme cluster tests for vim-core.
//!
//! Tests proper handling of multi-byte characters, combining marks,
//! emoji, and other Unicode edge cases.

use std::num::NonZeroU32;

use vim_core::commands::actions::{
    execute_increment_number, execute_toggle_case_char, toggle_case_range, ActionContext,
};
use vim_core::commands::CommandResult;
use vim_core::effects::Effect;
use vim_core::primitives::{LinewiseText, Offset, Range};

/// `execute_toggle_case_char` always emits a 3-effect undo group when the
/// cursor points at a character (BeginUndoGroup + Replace + EndUndoGroup).
/// Non-alphabetic characters emit a same-character Replace so the undo
/// group still records an edit (Neovim parity, src 3ad78f6d).
const TOGGLE_GROUP_LEN: usize = 3;

fn assert_toggle_group(result: &CommandResult) {
    let effects = result.effects.as_slice();
    assert_eq!(
        effects.len(),
        TOGGLE_GROUP_LEN,
        "expected BeginUndoGroup + Replace + EndUndoGroup, got {effects:?}",
    );
    assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
    assert!(matches!(effects[1], Effect::Replace { .. }));
    assert!(matches!(effects[2], Effect::EndUndoGroup { .. }));
}

const N1: NonZeroU32 = match NonZeroU32::new(1) {
    Some(v) => v,
    None => unreachable!(),
};

fn make_case_ctx(text: &str, cursor: usize) -> ActionContext<'_> {
    ActionContext::from_text_and_cursor(text, Offset::new(cursor), N1)
}

// =============================================================================
// Multi-byte Character Tests
// =============================================================================

#[test]
fn test_toggle_case_german_umlaut() {
    // ü -> Ü (2 bytes in UTF-8)
    let ctx = make_case_ctx("über", 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

#[test]
fn test_toggle_case_greek() {
    // α -> Α (2 bytes in UTF-8)
    let ctx = make_case_ctx("αβγ", 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

#[test]
fn test_toggle_case_cyrillic() {
    // а -> А
    let ctx = make_case_ctx("абв", 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

#[test]
fn test_toggle_case_chinese() {
    // Chinese characters have no case but still emit a same-char Replace
    // so the undo group records the cursor movement (src 3ad78f6d).
    let ctx = make_case_ctx("中文", 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

#[test]
fn test_toggle_case_japanese() {
    // Japanese katakana/hiragana have no case (same-char Replace; see above).
    let ctx = make_case_ctx("あいう", 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

#[test]
fn test_toggle_case_korean() {
    // Korean hangul has no case (same-char Replace; see above).
    let ctx = make_case_ctx("한글", 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

// =============================================================================
// Emoji Tests
// =============================================================================

#[test]
fn test_toggle_case_simple_emoji() {
    // Emoji have no case (same-char Replace for undo, src 3ad78f6d).
    let ctx = make_case_ctx("😀", 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

#[test]
fn test_toggle_case_emoji_zwj_sequence() {
    // Family emoji: 👨‍👩‍👧 (multiple code points joined with ZWJ).
    let ctx = make_case_ctx("👨\u{200D}👩\u{200D}👧", 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

#[test]
fn test_toggle_case_emoji_skin_tone() {
    // Emoji with skin tone modifier: 👍🏻
    let ctx = make_case_ctx("👍🏻", 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

#[test]
fn test_toggle_case_emoji_flag() {
    // Flag emoji: 🇺🇸 (regional indicator symbols)
    let ctx = make_case_ctx("🇺🇸", 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

// =============================================================================
// Combining Characters Tests
// =============================================================================

#[test]
fn test_toggle_case_combining_acute() {
    // e + combining acute = é (2 code points)
    let text = "e\u{0301}"; // e + combining acute accent
    let ctx = make_case_ctx(text, 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

#[test]
fn test_toggle_case_precomposed_accent() {
    // é (precomposed, single code point)
    let ctx = make_case_ctx("é", 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

#[test]
fn test_toggle_case_multiple_combining() {
    // Character with multiple combining marks
    let text = "a\u{0301}\u{0302}"; // a + acute + circumflex
    let ctx = make_case_ctx(text, 0);
    let result = execute_toggle_case_char(&ctx);
    // Should toggle the base character
    assert_toggle_group(&result);
}

// =============================================================================
// UTF-8 Byte Boundary Tests
// =============================================================================

#[test]
fn test_cursor_at_utf8_continuation_byte() {
    let text = "über"; // ü is 2 bytes: C3 BC
                       // Offset 1 is in the middle of ü - this is an invalid position
                       // The function should handle this gracefully
    let ctx = make_case_ctx(text, 1);
    // Should not panic, behavior is implementation-defined
    let _ = execute_toggle_case_char(&ctx);
}

#[test]
fn test_cursor_after_multibyte() {
    let text = "über"; // ü (2 bytes) + b + e + r
                       // After ü, which ends at byte 2
    let ctx = make_case_ctx(text, 2);
    let result = execute_toggle_case_char(&ctx);
    // Should toggle 'b'
    assert_toggle_group(&result);
}

#[test]
fn test_range_includes_multibyte() {
    let text = "über"; // ü (2 bytes) at positions 0-1
    let range = Range::from_raw(0, 4); // Include ü + be
    let result = toggle_case_range(text, range);
    assert_eq!(result.effects.len(), 1);
}

// =============================================================================
// LinewiseText Unicode Tests
// =============================================================================

#[test]
fn test_linewise_unicode_content() {
    let lw = LinewiseText::new("日本語");
    assert_eq!(lw.content(), "日本語");
}

#[test]
fn test_linewise_mixed_script() {
    let lw = LinewiseText::new("Hello 世界");
    assert_eq!(lw.content(), "Hello 世界");
}

#[test]
fn test_linewise_emoji_line() {
    let lw = LinewiseText::new("😀🎉");
    assert_eq!(lw.content(), "😀🎉");
}

#[test]
fn test_linewise_rtl_arabic() {
    let lw = LinewiseText::new("مرحبا");
    assert_eq!(lw.content(), "مرحبا");
}

#[test]
fn test_linewise_rtl_hebrew() {
    let lw = LinewiseText::new("שלום");
    assert_eq!(lw.content(), "שלום");
}

// =============================================================================
// Number in Unicode Context
// =============================================================================

#[test]
fn test_number_after_unicode() {
    // execute_increment_number wraps its work in BeginUndoGroup + Replace
    // + SetCursor + EndUndoGroup (4 effects).
    let text = "日本語123";
    let ctx = make_case_ctx(text, 0);
    let result = execute_increment_number(&ctx);
    assert_eq!(result.effects.len(), 4);
}

#[test]
fn test_number_between_unicode() {
    let text = "前123后";
    let ctx = make_case_ctx(text, 0);
    let result = execute_increment_number(&ctx);
    assert_eq!(result.effects.len(), 4);
}

// =============================================================================
// Special Unicode Categories
// =============================================================================

#[test]
fn test_toggle_case_fullwidth() {
    // Fullwidth Latin letters: Ａ -> ａ
    let ctx = make_case_ctx("Ａ", 0);
    let result = execute_toggle_case_char(&ctx);
    // Fullwidth letters have case
    assert_toggle_group(&result);
}

#[test]
fn test_toggle_case_math_symbols() {
    // Mathematical symbols like ∑ have no case (same-char Replace, src 3ad78f6d).
    let ctx = make_case_ctx("∑", 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

#[test]
fn test_toggle_case_currency() {
    // Currency symbols like € have no case (same-char Replace, src 3ad78f6d).
    let ctx = make_case_ctx("€", 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

// =============================================================================
// Edge Cases with Unicode
// =============================================================================

#[test]
fn test_empty_string_unicode_context() {
    let ctx = make_case_ctx("", 0);
    let result = execute_toggle_case_char(&ctx);
    assert!(result.effects.is_empty());
}

#[test]
fn test_single_unicode_char() {
    let ctx = make_case_ctx("日", 0);
    let result = execute_toggle_case_char(&ctx);
    // No case but still emits same-char Replace (src 3ad78f6d).
    assert_toggle_group(&result);
}

#[test]
fn test_zero_width_joiner() {
    // ZWJ is invisible but takes space; no case but same-char Replace.
    let text = "\u{200D}a"; // ZWJ + 'a'
    let ctx = make_case_ctx(text, 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}

#[test]
fn test_zero_width_space() {
    // Zero-width space; no case but same-char Replace.
    let text = "\u{200B}a"; // ZWSP + 'a'
    let ctx = make_case_ctx(text, 0);
    let result = execute_toggle_case_char(&ctx);
    assert_toggle_group(&result);
}
