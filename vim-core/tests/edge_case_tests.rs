//! Edge case tests for vim-core.
//!
//! Tests edge cases: empty documents, EOF, single character, whitespace-only, etc.

use std::num::NonZeroU32;

use vim_core::commands::actions::{
    execute_decrement_number, execute_increment_number, execute_indent_lines,
    execute_outdent_lines, execute_toggle_case_char, toggle_case_range, ActionContext,
};
use vim_core::effects::Effect;
use vim_core::primitives::{LinewiseText, Offset, Range};

const N1: NonZeroU32 = match NonZeroU32::new(1) {
    Some(v) => v,
    None => unreachable!(),
};

fn make_case_ctx(text: &str, cursor: usize) -> ActionContext<'_> {
    ActionContext::from_text_and_cursor(text, Offset::new(cursor), N1)
}

// =============================================================================
// Empty Document Tests
// =============================================================================

#[test]
fn test_empty_doc_toggle_case() {
    let ctx = make_case_ctx("", 0);
    let result = execute_toggle_case_char(&ctx);
    assert!(result.effects.is_empty());
}

#[test]
fn test_empty_doc_toggle_case_range() {
    let range = Range::from_raw(0, 0);
    let result = toggle_case_range("", range);
    assert!(result.effects.is_empty());
}

#[test]
fn test_empty_doc_increment() {
    let ctx = make_case_ctx("", 0);
    let result = execute_increment_number(&ctx);
    assert!(result.effects.is_empty());
}

#[test]
fn test_empty_doc_indent() {
    let ctx = make_case_ctx("", 0);
    let result = execute_indent_lines(&ctx);
    // Empty doc gets indent at pos 0 — should have BeginUndoGroup, Insert, EndUndoGroup
    let effects = result.effects.as_slice();
    assert!(
        effects.len() >= 3,
        "expected undo group + insert effects, got {}",
        effects.len()
    );
    assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
    // Should contain an Insert effect with indent text at offset 0
    let has_insert = effects
        .iter()
        .any(|e| matches!(e, Effect::Insert { offset, .. } if offset.get() == 0));
    assert!(has_insert, "expected Insert at offset 0 for indent");
    assert!(effects
        .iter()
        .any(|e| matches!(e, Effect::EndUndoGroup { .. })));
}

#[test]
fn test_empty_doc_outdent() {
    let ctx = make_case_ctx("", 0);
    let result = execute_outdent_lines(&ctx);
    assert!(result.effects.is_empty());
}

#[test]
fn test_empty_doc_linewise_text() {
    let lw = LinewiseText::new("");
    // LinewiseText invariant: always ends with newline
    assert!(
        lw.as_str().ends_with('\n'),
        "empty input should produce trailing newline"
    );
}

// =============================================================================
// Single Character Tests
// =============================================================================

#[test]
fn test_single_char_toggle_case_lower() {
    let ctx = make_case_ctx("a", 0);
    let result = execute_toggle_case_char(&ctx);
    // BeginUndoGroup + Replace('a' -> 'A') + EndUndoGroup
    assert_eq!(result.effects.len(), 3);
    assert!(matches!(
        result.effects.as_slice()[0],
        Effect::BeginUndoGroup { .. }
    ));
    assert!(matches!(
        result.effects.as_slice()[1],
        Effect::Replace { .. }
    ));
    assert!(matches!(
        result.effects.as_slice()[2],
        Effect::EndUndoGroup { .. }
    ));
}

#[test]
fn test_single_char_toggle_case_upper() {
    let ctx = make_case_ctx("A", 0);
    let result = execute_toggle_case_char(&ctx);
    // BeginUndoGroup + Replace('A' -> 'a') + EndUndoGroup
    assert_eq!(result.effects.len(), 3);
    assert!(matches!(
        result.effects.as_slice()[0],
        Effect::BeginUndoGroup { .. }
    ));
    assert!(matches!(
        result.effects.as_slice()[1],
        Effect::Replace { .. }
    ));
    assert!(matches!(
        result.effects.as_slice()[2],
        Effect::EndUndoGroup { .. }
    ));
}

#[test]
fn test_single_char_toggle_case_digit() {
    let ctx = make_case_ctx("5", 0);
    let result = execute_toggle_case_char(&ctx);
    // Non-alpha emits same-char Replace for undo tracking (Neovim parity, src 3ad78f6d).
    // BeginUndoGroup + Replace('5' -> '5') + EndUndoGroup
    assert_eq!(result.effects.len(), 3);
    assert!(matches!(
        result.effects.as_slice()[0],
        Effect::BeginUndoGroup { .. }
    ));
    assert!(matches!(
        result.effects.as_slice()[1],
        Effect::Replace { .. }
    ));
    assert!(matches!(
        result.effects.as_slice()[2],
        Effect::EndUndoGroup { .. }
    ));
    assert_eq!(result.cursor.unwrap().get(), 1);
}

#[test]
fn test_single_char_increment() {
    let ctx = make_case_ctx("5", 0);
    let result = execute_increment_number(&ctx);
    // Should replace "5" with "6": BeginUndoGroup, Replace, SetCursor, EndUndoGroup
    let effects = result.effects.as_slice();
    assert_eq!(
        effects.len(),
        4,
        "expected 4 effects (undo group + replace + cursor)"
    );
    assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
    match &effects[1] {
        Effect::Replace { range, text } => {
            assert_eq!(range.start().get(), 0);
            assert_eq!(range.end().get(), 1);
            assert_eq!(text.as_str(), "6");
        }
        other => panic!("expected Replace effect, got {:?}", other),
    }
    // SetCursor should point to the last digit of the new number
    match &effects[2] {
        Effect::SetCursor { offset } => assert_eq!(offset.get(), 0, "cursor on last digit of '6'"),
        other => panic!("expected SetCursor effect, got {:?}", other),
    }
    assert!(matches!(effects[3], Effect::EndUndoGroup { .. }));
}

#[test]
fn test_single_char_linewise_text() {
    let lw = LinewiseText::new("x");
    assert_eq!(
        lw.as_str(),
        "x\n",
        "single char should get trailing newline"
    );
}

// =============================================================================
// EOF (End of File) Tests
// =============================================================================

#[test]
fn test_eof_toggle_case() {
    let ctx = make_case_ctx("hello", 5); // At EOF
    let result = execute_toggle_case_char(&ctx);
    assert!(result.effects.is_empty());
}

#[test]
fn test_eof_increment() {
    let ctx = ActionContext::from_text_and_cursor("no numbers here", Offset::new(15), N1);
    let result = execute_increment_number(&ctx);
    assert!(result.effects.is_empty());
}

#[test]
fn test_cursor_beyond_text() {
    let ctx = make_case_ctx("abc", 100);
    let result = execute_toggle_case_char(&ctx);
    assert!(result.effects.is_empty());
}

// =============================================================================
// Whitespace-Only Tests
// =============================================================================

#[test]
fn test_whitespace_only_toggle_case() {
    let ctx = make_case_ctx("   ", 0);
    let result = execute_toggle_case_char(&ctx);
    // Non-alpha (space) emits same-char Replace for undo tracking (src 3ad78f6d).
    assert_eq!(result.effects.len(), 3);
    assert!(matches!(
        result.effects.as_slice()[0],
        Effect::BeginUndoGroup { .. }
    ));
    assert!(matches!(
        result.effects.as_slice()[1],
        Effect::Replace { .. }
    ));
    assert!(matches!(
        result.effects.as_slice()[2],
        Effect::EndUndoGroup { .. }
    ));
    assert_eq!(result.cursor.unwrap().get(), 1);
}

#[test]
fn test_whitespace_only_increment() {
    let ctx = make_case_ctx("   ", 0);
    let result = execute_increment_number(&ctx);
    assert!(result.effects.is_empty());
}

#[test]
fn test_whitespace_only_outdent() {
    let ctx = make_case_ctx("    ", 0);
    let result = execute_outdent_lines(&ctx);
    // Outdent 4 spaces: should produce BeginUndoGroup, Delete (removing spaces), EndUndoGroup
    let effects = result.effects.as_slice();
    assert!(
        effects.len() >= 3,
        "expected undo group + delete effects, got {}",
        effects.len()
    );
    assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
    let has_delete = effects
        .iter()
        .any(|e| matches!(e, Effect::Delete { range } if range.start().get() == 0));
    assert!(
        has_delete,
        "expected Delete effect removing leading whitespace"
    );
    assert!(effects
        .iter()
        .any(|e| matches!(e, Effect::EndUndoGroup { .. })));
}

#[test]
fn test_tabs_outdent() {
    let ctx = make_case_ctx("\thello", 0);
    let result = execute_outdent_lines(&ctx);
    // Tab is not space, so no outdent
    assert!(result.effects.is_empty());
}

// =============================================================================
// Newline-Only Tests
// =============================================================================

#[test]
fn test_newline_only_toggle_case() {
    let ctx = make_case_ctx("\n", 0);
    let result = execute_toggle_case_char(&ctx);
    // Non-alpha (newline) emits same-char Replace for undo tracking (src 3ad78f6d).
    assert_eq!(result.effects.len(), 3);
}

#[test]
fn test_newline_in_middle() {
    let ctx = make_case_ctx("a\nb", 1); // At newline
    let result = execute_toggle_case_char(&ctx);
    // Non-alpha (newline) emits same-char Replace for undo tracking (src 3ad78f6d).
    assert_eq!(result.effects.len(), 3);
}

// =============================================================================
// Boundary Condition Tests
// =============================================================================

#[test]
fn test_cursor_at_zero() {
    let ctx = make_case_ctx("hello", 0);
    let result = execute_toggle_case_char(&ctx);
    // BeginUndoGroup + Replace + EndUndoGroup
    assert_eq!(result.effects.len(), 3);
}

#[test]
fn test_cursor_at_last_char() {
    let ctx = make_case_ctx("hello", 4); // 'o'
    let result = execute_toggle_case_char(&ctx);
    // BeginUndoGroup + Replace + EndUndoGroup
    assert_eq!(result.effects.len(), 3);
}

#[test]
fn test_range_exceeds_text() {
    let text = "hi";
    let range = Range::from_raw(0, 100);
    let result = toggle_case_range(text, range);
    assert_eq!(result.effects.len(), 1);
}

#[test]
fn test_range_start_beyond_text() {
    let text = "hi";
    let range = Range::from_raw(10, 20);
    let result = toggle_case_range(text, range);
    assert!(result.effects.is_empty());
}

#[test]
fn test_zero_length_range() {
    let text = "hello";
    let range = Range::from_raw(2, 2);
    let result = toggle_case_range(text, range);
    assert!(result.effects.is_empty());
}

// =============================================================================
// Number Edge Cases
// =============================================================================

#[test]
fn test_increment_zero() {
    let ctx = make_case_ctx("0", 0);
    let result = execute_increment_number(&ctx);
    // 0 -> 1: BeginUndoGroup, Replace("0" -> "1"), SetCursor, EndUndoGroup
    let effects = result.effects.as_slice();
    assert_eq!(effects.len(), 4);
    let replace = effects.iter().find(|e| matches!(e, Effect::Replace { .. }));
    match replace {
        Some(Effect::Replace { text, .. }) => assert_eq!(text.as_str(), "1"),
        _ => panic!("expected Replace effect with '1'"),
    }
}

#[test]
fn test_increment_negative() {
    let ctx = make_case_ctx("-1", 0);
    let result = execute_increment_number(&ctx);
    // -1 -> 0: should have Replace effect
    let effects = result.effects.as_slice();
    assert_eq!(
        effects.len(),
        4,
        "expected undo group + replace + cursor effects"
    );
    let replace = effects.iter().find(|e| matches!(e, Effect::Replace { .. }));
    match replace {
        Some(Effect::Replace { text, .. }) => assert_eq!(text.as_str(), "0"),
        _ => panic!("expected Replace effect with '0'"),
    }
}

#[test]
fn test_increment_large_count() {
    let ctx =
        ActionContext::from_text_and_cursor("10", Offset::new(0), NonZeroU32::new(100).unwrap());
    let result = execute_increment_number(&ctx);
    // 10 + 100 = 110: should produce Replace with "110"
    let effects = result.effects.as_slice();
    assert_eq!(effects.len(), 4);
    let replace = effects.iter().find(|e| matches!(e, Effect::Replace { .. }));
    match replace {
        Some(Effect::Replace { text, .. }) => assert_eq!(text.as_str(), "110"),
        _ => panic!("expected Replace effect with '110'"),
    }
}

#[test]
fn test_decrement_to_negative() {
    let ctx =
        ActionContext::from_text_and_cursor("5", Offset::new(0), NonZeroU32::new(10).unwrap());
    let result = execute_decrement_number(&ctx);
    // 5 - 10 = -5: should produce Replace with "-5"
    let effects = result.effects.as_slice();
    assert_eq!(effects.len(), 4);
    let replace = effects.iter().find(|e| matches!(e, Effect::Replace { .. }));
    match replace {
        Some(Effect::Replace { text, .. }) => assert_eq!(text.as_str(), "-5"),
        _ => panic!("expected Replace effect with '-5'"),
    }
    // SetCursor should be on last digit of "-5" (offset 1)
    let set_cursor = effects
        .iter()
        .find(|e| matches!(e, Effect::SetCursor { .. }));
    match set_cursor {
        Some(Effect::SetCursor { offset }) => {
            assert_eq!(offset.get(), 1, "cursor on last digit of '-5'")
        }
        _ => panic!("expected SetCursor effect"),
    }
}

#[test]
fn test_hex_increment() {
    let ctx = make_case_ctx("0xff", 0);
    let result = execute_increment_number(&ctx);
    // 0xff + 1 = 0x100: should produce Replace effect
    let effects = result.effects.as_slice();
    assert_eq!(effects.len(), 4);
    let replace = effects.iter().find(|e| matches!(e, Effect::Replace { .. }));
    match replace {
        Some(Effect::Replace { text, .. }) => {
            assert!(
                text.as_str().starts_with("0x"),
                "hex increment should preserve 0x prefix, got: {}",
                text
            );
        }
        _ => panic!("expected Replace effect for hex increment"),
    }
}

#[test]
fn test_octal_increment() {
    let ctx = make_case_ctx("0o77", 0);
    let result = execute_increment_number(&ctx);
    // Engine finds "0" at cursor and increments it; 0o prefix not fully parsed as octal literal.
    // The key assertion: a Replace effect is produced with a valid number.
    let effects = result.effects.as_slice();
    assert_eq!(effects.len(), 4);
    assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
    let replace = effects.iter().find(|e| matches!(e, Effect::Replace { .. }));
    assert!(
        replace.is_some(),
        "expected Replace effect for number increment"
    );
    assert!(effects
        .iter()
        .any(|e| matches!(e, Effect::SetCursor { .. })));
}

#[test]
fn test_binary_increment() {
    let ctx = make_case_ctx("0b1111", 0);
    let result = execute_increment_number(&ctx);
    // 0b1111 + 1 = 0b10000: should produce Replace effect preserving 0b prefix
    let effects = result.effects.as_slice();
    assert_eq!(effects.len(), 4);
    let replace = effects.iter().find(|e| matches!(e, Effect::Replace { .. }));
    match replace {
        Some(Effect::Replace { text, .. }) => {
            assert!(
                text.as_str().starts_with("0b"),
                "binary increment should preserve 0b prefix, got: {}",
                text
            );
        }
        _ => panic!("expected Replace effect for binary increment"),
    }
}

#[test]
fn test_number_in_text() {
    let ctx = ActionContext::from_text_and_cursor("value = 42;", Offset::new(8), N1);
    let result = execute_increment_number(&ctx);
    // 42 -> 43: Replace should target byte range 8..10 and produce "43"
    let effects = result.effects.as_slice();
    assert_eq!(effects.len(), 4);
    let replace = effects.iter().find(|e| matches!(e, Effect::Replace { .. }));
    match replace {
        Some(Effect::Replace { range, text }) => {
            assert_eq!(
                range.start().get(),
                8,
                "replace should start at number position"
            );
            assert_eq!(range.end().get(), 10, "replace should end after '42'");
            assert_eq!(text.as_str(), "43");
        }
        _ => panic!("expected Replace effect with '43'"),
    }
}

#[test]
fn test_number_at_end() {
    let ctx = ActionContext::from_text_and_cursor("count: 99", Offset::new(7), N1);
    let result = execute_increment_number(&ctx);
    // 99 -> 100: Replace should produce "100"
    let effects = result.effects.as_slice();
    assert_eq!(effects.len(), 4);
    let replace = effects.iter().find(|e| matches!(e, Effect::Replace { .. }));
    match replace {
        Some(Effect::Replace { text, .. }) => assert_eq!(text.as_str(), "100"),
        _ => panic!("expected Replace effect with '100'"),
    }
}

#[test]
fn test_no_number_in_text() {
    // Use text without "no"/"yes"/"true"/"false" keywords (boolean toggle triggers on those)
    let ctx = make_case_ctx("abc xyz", 0);
    let result = execute_increment_number(&ctx);
    assert!(
        result.effects.is_empty(),
        "expected no effects when no number or boolean keyword is present"
    );
}

#[test]
fn test_number_with_leading_zeros() {
    let ctx = make_case_ctx("007", 0);
    let result = execute_increment_number(&ctx);
    // 007 -> 008: Replace should produce a result (leading zeros behavior varies)
    let effects = result.effects.as_slice();
    assert_eq!(
        effects.len(),
        4,
        "expected undo group + replace + cursor effects"
    );
    assert!(matches!(effects[0], Effect::BeginUndoGroup { .. }));
    let replace = effects.iter().find(|e| matches!(e, Effect::Replace { .. }));
    assert!(
        replace.is_some(),
        "expected Replace effect for number increment"
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::SetCursor { .. })),
        "expected SetCursor effect"
    );
}

// =============================================================================
// LinewiseText Edge Cases
// =============================================================================

#[test]
fn test_linewise_trailing_newline() {
    let lw = LinewiseText::new("hello\n");
    assert_eq!(
        lw.as_str(),
        "hello\n",
        "already has trailing newline — should be unchanged"
    );
}

#[test]
fn test_linewise_multiple_empty_lines() {
    let lw = LinewiseText::new("\n\n\n");
    let s = lw.as_str();
    assert!(s.ends_with('\n'), "must end with newline, got {:?}", s);
    assert!(!s.is_empty(), "must not be empty");
}

#[test]
fn test_linewise_long_line() {
    let long = "a".repeat(10000);
    let lw = LinewiseText::new(&long);
    let s = lw.as_str();
    assert!(s.ends_with('\n'), "long line must get trailing newline");
    assert_eq!(s.len(), 10001, "should be original length + 1 newline");
}

#[test]
fn test_linewise_mixed_newlines() {
    let lw = LinewiseText::new("line1\nline2\nline3\n");
    assert_eq!(
        lw.as_str(),
        "line1\nline2\nline3\n",
        "already ends with newline — unchanged"
    );
}

// =============================================================================
// Range Edge Cases
// =============================================================================

#[test]
fn test_range_empty() {
    let range = Range::from_raw(0, 0);
    assert!(range.is_empty());
    assert_eq!(range.len(), 0);
}

#[test]
fn test_range_single_byte() {
    let range = Range::from_raw(0, 1);
    assert!(!range.is_empty());
    assert_eq!(range.len(), 1);
}

#[test]
fn test_range_contains() {
    let range = Range::from_raw(5, 10);
    assert!(range.contains(Offset::new(5)));
    assert!(range.contains(Offset::new(9)));
    assert!(!range.contains(Offset::new(10))); // exclusive end
    assert!(!range.contains(Offset::new(4)));
}

#[test]
fn test_range_overlaps() {
    let a = Range::from_raw(0, 10);
    let b = Range::from_raw(5, 15);
    let c = Range::from_raw(10, 20);
    assert!(a.overlaps(b));
    assert!(b.overlaps(a));
    assert!(!a.overlaps(c)); // end is exclusive
}

// =============================================================================
// Offset Edge Cases
// =============================================================================

#[test]
fn test_offset_zero() {
    let offset = Offset::new(0);
    assert_eq!(offset.get(), 0);
}

#[test]
fn test_offset_max() {
    // Offset::MAX is usize::MAX - 1 (usize::MAX is reserved as the niche sentinel
    // for Option<Offset> to achieve 8-byte representation).
    let offset = Offset::MAX;
    assert_eq!(offset.get(), usize::MAX - 1);
}

#[test]
#[should_panic(expected = "usize::MAX is not a valid Offset")]
fn test_offset_usize_max_panics() {
    let _ = Offset::new(usize::MAX);
}

#[test]
fn test_offset_ordering() {
    let a = Offset::new(5);
    let b = Offset::new(10);
    assert!(a < b);
    assert!(b > a);
}

#[test]
fn test_offset_equality() {
    let a = Offset::new(5);
    let b = Offset::new(5);
    assert_eq!(a, b);
}
