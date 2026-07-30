//! Case operators (gu, gU, g~).
//!
//! Transform text case within a range.
//!
//! # Behavior
//!
//! - `guw` - Lowercase word
//! - `gUU` - Uppercase line
//! - `g~iw` - Toggle case of word

use super::types::{extract_range_text, CaseTransform, OperatorContext};
use crate::commands::helpers::toggle_case_chars;
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::{MarkName, Offset};
use compact_str::CompactString;

/// Execute lowercase operator (gu).
///
/// No traits, just functions + enum dispatch.
pub fn execute_lowercase(ctx: &OperatorContext<'_>) -> CommandResult {
    execute_case_change(ctx, CaseTransform::Lower)
}

/// Execute uppercase operator (gU).
///
/// No traits, just functions + enum dispatch.
pub fn execute_uppercase(ctx: &OperatorContext<'_>) -> CommandResult {
    execute_case_change(ctx, CaseTransform::Upper)
}

/// Execute toggle case operator (g~).
///
/// No traits, just functions + enum dispatch.
pub fn execute_toggle_case(ctx: &OperatorContext<'_>) -> CommandResult {
    execute_case_change(ctx, CaseTransform::Toggle)
}

/// Execute ROT13 cipher operator (g?).
///
/// No traits, just functions + enum dispatch.
pub fn execute_rot13(ctx: &OperatorContext<'_>) -> CommandResult {
    execute_case_change(ctx, CaseTransform::Rot13)
}

/// Execute ROT47 cipher operator (g&).
///
/// No traits, just functions + enum dispatch.
pub fn execute_rot47(ctx: &OperatorContext<'_>) -> CommandResult {
    execute_case_change(ctx, CaseTransform::Rot47)
}

/// Apply ROT13 cipher to a string: a-z rotated by 13, A-Z rotated by 13, others unchanged.
pub(crate) fn rot13_chars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    for ch in input.chars() {
        let rotated = match ch {
            'a'..='z' => (b'a' + (ch as u8 - b'a' + 13) % 26) as char,
            'A'..='Z' => (b'A' + (ch as u8 - b'A' + 13) % 26) as char,
            _ => ch,
        };
        result.push(rotated);
    }
    result
}

/// Apply ROT47 cipher to a single character.
///
/// For ASCII code points 33-126 ('!' through '~'), rotates by 47 positions
/// within that 94-character range. Characters outside this range are unchanged.
/// ROT47 is its own inverse (applying twice returns the original).
pub(crate) const fn rot47_char(ch: char) -> char {
    match ch {
        '!'..='~' => (b'!' + (ch as u8 - b'!' + 47) % 94) as char,
        _ => ch,
    }
}

/// Apply ROT47 cipher to a string: all printable ASCII (33-126) rotated by 47.
pub(crate) fn rot47_chars(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    for ch in input.chars() {
        result.push(rot47_char(ch));
    }
    result
}

/// Execute case transformation using Effects builder pattern.
fn execute_case_change(ctx: &OperatorContext<'_>, transform: CaseTransform) -> CommandResult {
    if ctx.is_empty() {
        // Neovim's op_tilde internally does dec(&oap->end) for exclusive motions.
        // When the range is empty (start == end, from exclusive motion that didn't
        // move), dec at buffer start is a no-op, so op_tilde still processes 1 char.
        // Only bail out early for truly empty buffers — non-empty buffers with a
        // zero-width charwise range should extend by 1 char below (matching `gu0`
        // at column 0 lowercasing the character under cursor).
        let can_extend = ctx.motion_type == crate::primitives::MotionType::CharWise
            && ctx.range.start().get() < ctx.text.len();
        if !can_extend {
            // Neovim always sets `[` and `]` marks and creates an undo entry,
            // even when the range is empty (e.g. `gUU` on an empty buffer).
            let effects = Effects::new()
                .begin_undo()
                .set_mark(MarkName::CHANGE_START, ctx.cursor, None)
                .set_mark(MarkName::CHANGE_END, ctx.cursor, None)
                .set_cursor(ctx.cursor)
                .end_undo();
            return CommandResult::new(effects, ctx.cursor);
        }
    }

    // Neovim's op_tilde internally does dec(&oap->end) for exclusive motions.
    // When the range is empty (start == end, from exclusive motion that didn't
    // move), dec at buffer start is a no-op, so op_tilde still processes 1 char.
    let effective_range = if ctx.range.is_empty()
        && ctx.motion_type == crate::primitives::MotionType::CharWise
        && ctx.range.start().get() < ctx.text.len()
    {
        let start = ctx.range.start().get();
        let char_len = ctx.text[start..].chars().next().map_or(0, char::len_utf8);
        crate::primitives::Range::from_raw(start, start + char_len)
    } else {
        ctx.range
    };

    // Get text in range
    let original = extract_range_text(ctx.text, effective_range);

    // Transform the text
    let transformed: String = match transform {
        CaseTransform::Lower => original.to_lowercase(),
        CaseTransform::Upper => original.to_uppercase(),
        CaseTransform::Toggle => toggle_case_chars(&original),
        CaseTransform::Rot13 => rot13_chars(&original),
        CaseTransform::Rot47 => rot47_chars(&original),
    };

    // Cursor goes to oap->start = min(cursor, motion_target).
    // For charwise, range.start() already equals this.
    // For linewise (guu/gUU/guk etc.), Neovim sets cursor to oap->start
    // before op_tilde runs. op_tilde does NOT move the cursor.
    let new_cursor = if ctx.motion_type == crate::primitives::MotionType::LineWise {
        Offset::new(ctx.cursor.get().min(ctx.motion_target.get()))
    } else {
        Offset::new(ctx.range.start().get())
    };

    // Neovim always sets `[` and `]` marks to the operator range,
    // even when the text doesn't actually change (e.g. `guu` on
    // already-lowercase text).
    let range_start = effective_range.start();
    // `]` is inclusive: last byte of range (subtract 1 from exclusive end).
    // For linewise ranges ending with '\n', point to last char before '\n'.
    let range_end_inclusive = {
        let end = effective_range
            .start()
            .saturating_add_raw(effective_range.len());
        if transformed.ends_with('\n') && effective_range.len() >= 2 {
            end.saturating_sub_raw(2)
        } else if !effective_range.is_empty() {
            end.saturating_sub_raw(1)
        } else {
            end
        }
    };

    let effects = if transformed == original.as_str() {
        // No visible text change, but Neovim records an undo entry anyway.
        // Wrap in undo group (no Replace needed — undo tree now records
        // empty groups) so 'u' undoes this cursor movement.
        Effects::new()
            .begin_undo()
            .set_mark(MarkName::CHANGE_START, range_start, None)
            .set_mark(MarkName::CHANGE_END, range_end_inclusive, None)
            .set_cursor(new_cursor)
            .end_undo()
    } else {
        Effects::new()
            .begin_undo()
            .replace(effective_range, CompactString::new(&transformed))
            .set_cursor(new_cursor)
            .end_undo()
    };

    CommandResult::new(effects, new_cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;
    use crate::primitives::{MotionType, Offset, Range};

    #[test]
    fn test_lowercase() {
        let ctx = OperatorContext::new(
            "HELLO WORLD",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute_lowercase(&ctx);

        let has_replace = result.effects.iter().any(|e| {
            if let Effect::Replace { text, .. } = e {
                text.as_str() == "hello"
            } else {
                false
            }
        });
        assert!(has_replace, "Lowercase should replace with lowercased text");
    }

    #[test]
    fn test_uppercase() {
        let ctx = OperatorContext::new(
            "hello world",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute_uppercase(&ctx);

        let has_replace = result.effects.iter().any(|e| {
            if let Effect::Replace { text, .. } = e {
                text.as_str() == "HELLO"
            } else {
                false
            }
        });
        assert!(has_replace, "Uppercase should replace with uppercased text");
    }

    #[test]
    fn test_toggle_case() {
        let ctx = OperatorContext::new(
            "HeLLo",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute_toggle_case(&ctx);

        let has_replace = result.effects.iter().any(|e| {
            if let Effect::Replace { text, .. } = e {
                text.as_str() == "hEllO"
            } else {
                false
            }
        });
        assert!(has_replace, "Toggle case should swap each character's case");
    }

    #[test]
    fn test_rot13() {
        let ctx = OperatorContext::new(
            "Hello, World!",
            Range::from_raw(0, 13),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute_rot13(&ctx);

        let has_replace = result.effects.iter().any(|e| {
            if let Effect::Replace { text, .. } = e {
                text.as_str() == "Uryyb, Jbeyq!"
            } else {
                false
            }
        });
        assert!(
            has_replace,
            "ROT13 should encode 'Hello, World!' as 'Uryyb, Jbeyq!'"
        );
    }

    #[test]
    fn test_rot13_involution() {
        // ROT13 applied twice should return the original text
        let text = "The Quick Brown Fox";
        let once = rot13_chars(text);
        let twice = rot13_chars(&once);
        assert_eq!(twice, text, "ROT13 applied twice should equal original");
    }

    #[test]
    fn test_no_change_needed() {
        let ctx = OperatorContext::new(
            "hello",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute_lowercase(&ctx);

        // Already lowercase: no Replace needed, but undo group IS emitted
        // (Neovim records the cursor movement as an undoable operation).
        let has_replace = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Replace { .. }));
        assert!(!has_replace, "Should not replace if text unchanged");

        let has_undo = result
            .effects
            .iter()
            .any(|e| matches!(e, Effect::BeginUndoGroup { .. }));
        assert!(
            has_undo,
            "Should have undo group (Neovim records even no-change ops)"
        );
    }

    #[test]
    fn test_rot47_char_h() {
        // 'H' (72) → '!' + (72-33+47)%94 = '!' + 86 = 119 = 'w'
        assert_eq!(rot47_char('H'), 'w');
    }

    #[test]
    fn test_rot47_char_e() {
        // 'e' (101) → '!' + (101-33+47)%94 = '!' + 21 = 54 = '6'
        assert_eq!(rot47_char('e'), '6');
    }

    #[test]
    fn test_rot47_char_l() {
        // 'l' (108) → '!' + (108-33+47)%94 = '!' + 28 = 61 = '='
        assert_eq!(rot47_char('l'), '=');
    }

    #[test]
    fn test_rot47_char_outside_range() {
        // Characters outside printable ASCII 33-126 are unchanged
        assert_eq!(rot47_char(' '), ' '); // space (32)
        assert_eq!(rot47_char('\n'), '\n');
        assert_eq!(rot47_char('\t'), '\t');
    }

    #[test]
    fn test_rot47_involution() {
        // ROT47 applied twice should return the original text
        let text = "Hello, World! 123 @#$";
        let once = rot47_chars(text);
        let twice = rot47_chars(&once);
        assert_eq!(twice, text, "ROT47 applied twice should equal original");
    }

    #[test]
    fn test_rot47_operator() {
        let ctx = OperatorContext::new(
            "Hello",
            Range::from_raw(0, 5),
            MotionType::CharWise,
            None,
            1,
            Offset::new(0),
        );

        let result = execute_rot47(&ctx);

        let has_replace = result.effects.iter().any(|e| {
            if let Effect::Replace { text, .. } = e {
                text.as_str() == "w6==@"
            } else {
                false
            }
        });
        assert!(has_replace, "ROT47 should encode 'Hello' as 'w6==@'");
    }

    #[test]
    fn test_rot47_boundary_exclamation() {
        // '!' (33) is the first char in the ROT47 range.
        // '!' + 47 = 'P' (80)
        assert_eq!(rot47_char('!'), 'P');
        // And applying again returns '!'
        assert_eq!(rot47_char('P'), '!');
    }

    #[test]
    fn test_rot47_boundary_tilde() {
        // '~' (126) is the last char in the ROT47 range.
        // '!' + (126 - 33 + 47) % 94 = '!' + (93 + 47) % 94 = '!' + 46 = 'O' (79)
        assert_eq!(rot47_char('~'), 'O');
        // And applying again returns '~'
        assert_eq!(rot47_char('O'), '~');
    }

    #[test]
    fn test_rot47_non_ascii_passthrough() {
        // Non-ASCII characters (> 127) must pass through unchanged.
        assert_eq!(rot47_char('\u{00E9}'), '\u{00E9}'); // e-acute
        assert_eq!(rot47_char('\u{4E16}'), '\u{4E16}'); // CJK character
        assert_eq!(rot47_char('\u{1F600}'), '\u{1F600}'); // emoji

        // Full string with mixed ASCII and non-ASCII
        let input = "Hi \u{00E9}\u{4E16}!";
        let encoded = rot47_chars(input);
        // 'H'->w, 'i'->:, ' ' unchanged, non-ASCII unchanged, '!'->P
        assert_eq!(encoded, "w: \u{00E9}\u{4E16}P");
        // Involution holds for mixed content
        let decoded = rot47_chars(&encoded);
        assert_eq!(decoded, input);
    }

    #[test]
    fn test_rot47_involution_all_printable_ascii() {
        // Verify involution property holds for every char in the ROT47 range (33-126).
        for code in 33u8..=126u8 {
            let ch = code as char;
            let double = rot47_char(rot47_char(ch));
            assert_eq!(
                double, ch,
                "ROT47 involution failed for '{}' (code {})",
                ch, code
            );
        }
    }
}
