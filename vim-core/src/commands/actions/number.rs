//! Number increment/decrement actions (`Ctrl-A`, `Ctrl-X`).
//!
//! Increments or decrements the number under or after the cursor.
//! Implemented as plain functions, not trait methods — no dynamic dispatch.

use super::types::ActionContext;
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::byte_delta;
use crate::primitives::{Offset, Range};
use compact_str::CompactString;

/// A matched number found in the document text.
struct NumberMatch {
    /// Byte offset of the start of the number (including sign/prefix).
    start: usize,
    /// Byte offset past the end of the number.
    end: usize,
    /// Parsed numeric value (signed).
    value: i64,
    /// Numeric base (2, 10, or 16).
    base: u32,
    /// Number of digits after the prefix (for zero-padding preservation).
    digit_count: usize,
}

/// Find the number at or after cursor position.
///
/// Neovim default nrformats=bin,hex recognizes:
/// - `0x`/`0X` prefix for hex
/// - `0b`/`0B` prefix for binary
/// - Plain decimal (no `0o` octal — that's Rust, not Vim)
fn find_number(text: &str, from: usize) -> Option<NumberMatch> {
    let bytes = text.as_bytes();
    let len = bytes.len();

    if from >= len {
        return None;
    }

    // Vim scans backward first: if the cursor is on a digit (or hex digit following
    // a 0x prefix), the entire number containing that digit is used. Only if the
    // cursor is not on a number do we scan forward.
    let mut start = from;

    // Only enter backward scan if cursor is on a decimal digit, OR if it's on a
    // hex letter (a-f/A-F) and a 0x prefix exists nearby.  Without this guard,
    // hex letters like 'f' in "foo 42" would trigger the backward-scan path,
    // find no valid decimal number, and return None — skipping forward scan.
    let on_hex_letter = bytes
        .get(from)
        .is_some_and(|b| b.is_ascii_hexdigit() && !b.is_ascii_digit());
    let has_hex_prefix = on_hex_letter && {
        // Peek backward past hex digits to see if there's a 0x/0X prefix.
        let mut p = from;
        while p > 0 && bytes.get(p - 1).is_some_and(u8::is_ascii_hexdigit) {
            p -= 1;
        }
        p >= 2
            && bytes.get(p - 1).is_some_and(|b| matches!(b, b'x' | b'X'))
            && bytes.get(p - 2) == Some(&b'0')
    };

    if bytes.get(from).is_some_and(u8::is_ascii_digit) || has_hex_prefix {
        // Cursor is on a digit-like byte — scan backward to find number start.
        // Walk back over digits, then check for 0x/0b prefix and leading minus.
        while start > 0 && bytes.get(start - 1).is_some_and(u8::is_ascii_hexdigit) {
            start -= 1;
        }
        // Check for 0x/0b prefix
        if start >= 2
            && bytes
                .get(start - 1)
                .is_some_and(|b| matches!(b, b'x' | b'X' | b'b' | b'B'))
            && bytes.get(start - 2) == Some(&b'0')
        {
            start -= 2;
        }
    } else {
        // Not on a digit — scan forward to find the next digit
        while start < len {
            let Some(&b) = bytes.get(start) else { break };
            if b.is_ascii_digit() {
                break;
            }
            start += 1;
        }
        if start >= len {
            return None;
        }
    }

    // Check for leading minus sign
    let negative = start > 0 && bytes.get(start - 1) == Some(&b'-');
    if negative {
        start -= 1;
    }

    // Check for hex/binary prefix (Neovim: 0x for hex, 0b for binary; NO 0o for octal)
    let prefix_start = if negative { start + 1 } else { start };
    let (base, num_start) = if bytes.get(prefix_start) == Some(&b'0') {
        match bytes.get(prefix_start + 1) {
            Some(b'x' | b'X') => (16, prefix_start + 2),
            Some(b'b' | b'B') => (2, prefix_start + 2),
            _ => (10, prefix_start),
        }
    } else {
        (10, prefix_start)
    };

    // Find end of number
    let mut end = num_start;
    while end < len {
        let Some(&b) = bytes.get(end) else { break };
        let valid = match base {
            16 => b.is_ascii_hexdigit(),
            10 => b.is_ascii_digit(),
            2 => matches!(b, b'0' | b'1'),
            _ => false,
        };
        if !valid {
            break;
        }
        end += 1;
    }

    if end <= num_start {
        return None;
    }

    let digit_count = end - num_start;

    // Parse the number
    let num_str = &text[num_start..end];
    let abs_value = i64::from_str_radix(num_str, base).ok()?;
    let value = if negative { -abs_value } else { abs_value };

    Some(NumberMatch {
        start,
        end,
        value,
        base,
        digit_count,
    })
}

/// Format a number with the given base, prefix, and minimum digit width.
///
/// Neovim preserves the original digit count for hex/binary (zero-padding).
/// Uses `unsigned_abs()` to safely handle `i64::MIN` (where `-value` would overflow).
fn format_number(value: i64, base: u32, min_digits: usize) -> String {
    match base {
        16 => {
            if value < 0 {
                let abs = value.unsigned_abs();
                let digits = format!("{abs:x}");
                let width = digits.len().max(min_digits);
                format!("-0x{digits:0>width$}")
            } else {
                let digits = format!("{value:x}");
                let width = digits.len().max(min_digits);
                format!("0x{digits:0>width$}")
            }
        }
        2 => {
            if value < 0 {
                let abs = value.unsigned_abs();
                let digits = format!("{abs:b}");
                let width = digits.len().max(min_digits);
                format!("-0b{digits:0>width$}")
            } else {
                let digits = format!("{value:b}");
                let width = digits.len().max(min_digits);
                format!("0b{digits:0>width$}")
            }
        }
        _ => value.to_string(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Boolean toggle support
// ═══════════════════════════════════════════════════════════════════════════════

/// Boolean keyword pairs. Each entry is `(word, toggled_word)`.
/// Listed longest-first within each case group to prevent partial matches
/// (e.g. "false" before "true" avoids matching "true" inside "truefalse").
const BOOLEAN_PAIRS: &[(&str, &str)] = &[
    // lowercase
    ("false", "true"),
    ("true", "false"),
    ("yes", "no"),
    ("no", "yes"),
    ("on", "off"),
    ("off", "on"),
    // Title case
    ("False", "True"),
    ("True", "False"),
    ("Yes", "No"),
    ("No", "Yes"),
    ("On", "Off"),
    ("Off", "On"),
    // UPPERCASE
    ("FALSE", "TRUE"),
    ("TRUE", "FALSE"),
    ("YES", "NO"),
    ("NO", "YES"),
    ("ON", "OFF"),
    ("OFF", "ON"),
];

/// Find a boolean keyword at or after `cursor` on the current line of `text`.
///
/// Scans rightward from `cursor` within the same line. At each position,
/// checks all known boolean keywords (longest first to avoid partial matches).
///
/// Returns `(start_byte, end_byte, matched_word)` or `None`.
fn find_boolean(text: &str, cursor: usize) -> Option<(usize, usize, &str)> {
    if cursor >= text.len() {
        return None;
    }

    // Restrict search to the current line
    let line_end = text[cursor..]
        .find('\n')
        .map_or(text.len(), |pos| cursor + pos);

    let mut pos = cursor;
    while pos < line_end {
        // Try each boolean keyword at this position (longest first is ensured
        // by table order — "false" (5) before "true" (4), etc.)
        for &(keyword, _) in BOOLEAN_PAIRS {
            let end = pos + keyword.len();
            if end <= line_end && text.get(pos..end) == Some(keyword) {
                // Ensure we match a whole word: the character before `pos` (if any)
                // and the character after `end` (if any) must not be alphanumeric/underscore.
                let word_boundary_before = pos == 0
                    || text
                        .as_bytes()
                        .get(pos - 1)
                        .is_none_or(|&b| !b.is_ascii_alphanumeric() && b != b'_');
                let word_boundary_after = text
                    .as_bytes()
                    .get(end)
                    .is_none_or(|&b| !b.is_ascii_alphanumeric() && b != b'_');
                if word_boundary_before && word_boundary_after {
                    return Some((pos, end, keyword));
                }
            }
        }
        pos += 1;
    }

    None
}

/// Toggle a boolean keyword, preserving its case pattern.
///
/// Returns `None` if `word` is not a recognized boolean keyword.
fn toggle_boolean(word: &str) -> Option<CompactString> {
    for &(keyword, toggled) in BOOLEAN_PAIRS {
        if word == keyword {
            return Some(CompactString::from(toggled));
        }
    }
    None
}

/// Try to find and toggle a boolean keyword at or after cursor.
///
/// If found, returns `CommandResult` with Replace + SetCursor effects.
/// The cursor is placed on the last character of the toggled word.
fn try_boolean_toggle(ctx: &ActionContext<'_>) -> Option<CommandResult> {
    let cursor = ctx.cursor.get();
    let (start, end, keyword) = find_boolean(ctx.text, cursor)?;
    let toggled = toggle_boolean(keyword)?;

    let range = Range::from_raw(start, end);
    let new_cursor = Offset::new(start + toggled.len().saturating_sub(1));

    Some(CommandResult::effects_only(
        Effects::new()
            .begin_undo()
            .replace(range, toggled)
            .set_cursor(new_cursor)
            .end_undo(),
    ))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Number increment/decrement
// ═══════════════════════════════════════════════════════════════════════════════

/// Increment the number under or after cursor (`Ctrl-A`).
///
/// Finds the first number at or after cursor and increments by count.
///
/// # Arguments
/// * `ctx` - Action context with text, cursor, and count
///
/// # Returns
/// * `CommandResult` with effects to apply
pub fn execute_increment_number(ctx: &ActionContext<'_>) -> CommandResult {
    execute_change_number(ctx, i64::from(ctx.count))
}

/// Decrement the number under or after cursor (`Ctrl-X`).
///
/// Finds the first number at or after cursor and decrements by count.
///
/// # Arguments
/// * `ctx` - Action context with text, cursor, and count
///
/// # Returns
/// * `CommandResult` with effects to apply
pub fn execute_decrement_number(ctx: &ActionContext<'_>) -> CommandResult {
    execute_change_number(ctx, -i64::from(ctx.count))
}

/// Shared implementation for increment and decrement.
///
/// Tries boolean toggle first, then finds a number and applies the delta.
/// Positive `delta` increments, negative `delta` decrements.
///
/// In visual mode, increments ALL numbers in the selection by the same delta
/// and places cursor at the selection start.
fn execute_change_number(ctx: &ActionContext<'_>, delta: i64) -> CommandResult {
    // Visual mode: increment/decrement all numbers in the selection
    if let Some(selection) = ctx.selection {
        return execute_visual_change_number(ctx, selection, delta);
    }

    // Try boolean toggle first — both Ctrl-A and Ctrl-X toggle symmetrically
    if let Some(result) = try_boolean_toggle(ctx) {
        return result;
    }

    let cursor = ctx.cursor.get();

    let Some(m) = find_number(ctx.text, cursor) else {
        return CommandResult::empty(ctx.cursor);
    };

    let new_value = m.value.saturating_add(delta);
    let new_text = format_number(new_value, m.base, m.digit_count);
    let range = Range::from_raw(m.start, m.end);

    // Cursor on last character of new number (Vim places cursor ON last digit)
    let new_cursor = Offset::new(m.start + new_text.len().saturating_sub(1));

    CommandResult::effects_only(
        Effects::new()
            .begin_undo()
            .replace(range, new_text)
            .set_cursor(new_cursor)
            .end_undo(),
    )
}

/// Visual mode Ctrl-A/X: increment/decrement the FIRST number on each
/// selected line that starts within the selection bounds.
/// Cursor goes to selection start. Exits visual mode after the operation.
///
/// Neovim behavior: one number per line, first-found within selection bounds.
fn execute_visual_change_number(
    ctx: &ActionContext<'_>,
    selection: crate::primitives::SelectionRange,
    delta: i64,
) -> CommandResult {
    use crate::commands::helpers;
    use crate::primitives::Mode;

    let sel_start = selection.start().get();
    let sel_end = selection.end().get();

    // Find the first number on each selected line (within selection bounds).
    // Walk line by line through the selection.
    let mut numbers_to_change: Vec<NumberMatch> = Vec::new();
    let mut line_start = helpers::line_start_for_offset(ctx.text, sel_start);

    loop {
        // Effective search range for this line: max(line_start, sel_start)..min(line_end, sel_end)
        let search_start = line_start.max(sel_start);
        let line_end = ctx.text[line_start..]
            .find('\n')
            .map_or(ctx.text.len(), |i| line_start + i);
        let search_end = line_end.min(sel_end);

        if search_start <= search_end {
            // Find first number starting within this line's selection range
            if let Some(m) = find_number_in_range(ctx.text, search_start, search_end) {
                numbers_to_change.push(m);
            }
        }

        // Move to next line
        if line_end >= ctx.text.len() || line_end >= sel_end {
            break;
        }
        line_start = line_end + 1;
    }

    if numbers_to_change.is_empty() {
        let effects = Effects::new()
            .clear_selection()
            .set_mode(Mode::Normal)
            .set_cursor(Offset::new(sel_start));
        return CommandResult::effects_only(effects);
    }

    let mut effects = Effects::new().begin_undo();

    // Process from end to start for stable offsets
    for m in numbers_to_change.iter().rev() {
        let new_value = m.value.saturating_add(delta);
        let new_text = format_number(new_value, m.base, m.digit_count);
        effects = effects.replace(Range::from_raw(m.start, m.end), new_text);
    }

    // Explicit change marks: [=first number start, ]=last number end.
    // Since we process bottom-to-top (rev) for stable offsets,
    // sync_change_marks would record the wrong order. Override explicitly.
    //
    // Neovim's op_addsub calls changed_lines(start.lnum, 0, ...) after
    // all changes, which sets mark '.' to (start.lnum, col=0) = line start
    // of the first selected line. Ref: ops.c:2380
    // numbers_to_change is guaranteed non-empty (early return above).
    #[allow(
        clippy::indexing_slicing,
        reason = "guaranteed non-empty by early return"
    )]
    let first_number = &numbers_to_change[0];
    #[allow(
        clippy::indexing_slicing,
        reason = "guaranteed non-empty by early return"
    )]
    let last_number = &numbers_to_change[numbers_to_change.len() - 1];
    let last_new_text = format_number(
        last_number.value.saturating_add(delta),
        last_number.base,
        last_number.digit_count,
    );
    let mark_start = Offset::new(first_number.start);
    // Compute cumulative shift from replacements before the last number.
    // Each replacement changes the document length by (new_len - old_len).
    let cumulative_shift: isize = numbers_to_change
        .iter()
        .take(numbers_to_change.len() - 1)
        .map(|m| {
            let new_text = format_number(m.value.saturating_add(delta), m.base, m.digit_count);
            byte_delta::delta(new_text.len(), m.end - m.start)
        })
        .sum();
    let last_new_start = last_number.start.saturating_add_signed(cumulative_shift);
    let mark_end = Offset::new(last_new_start + last_new_text.len().saturating_sub(1));
    let mark_dot = Offset::new(helpers::line_start_for_offset(ctx.text, sel_start));
    effects = effects
        .set_mark(crate::primitives::MarkName::CHANGE_START, mark_start, None)
        .set_mark(crate::primitives::MarkName::CHANGE_END, mark_end, None)
        .set_mark(crate::primitives::MarkName::LAST_CHANGE, mark_dot, None);

    let cursor = Offset::new(sel_start);
    CommandResult::effects_only(
        effects
            .set_cursor(cursor)
            .end_undo()
            .clear_selection()
            .set_mode(Mode::Normal),
    )
}

/// Find the first number within the byte range [start, end] of the text.
///
/// Unlike `find_number`, this constrains the search to the given range:
/// the number must start within the range and only digits within the range
/// are considered part of the number. This matches Neovim's visual Ctrl-A
/// behavior where e.g. selecting just "9" of "999" increments only that "9".
fn find_number_in_range(text: &str, start: usize, end: usize) -> Option<NumberMatch> {
    // Extract the substring for the selection range (inclusive end)
    let range_end = (end + 1).min(text.len());
    let slice = &text[start..range_end];

    // Find a number within the slice, then adjust offsets back to absolute
    let m = find_number(slice, 0)?;
    Some(NumberMatch {
        start: m.start + start,
        end: m.end + start,
        value: m.value,
        base: m.base,
        digit_count: m.digit_count,
    })
}

/// Sequential increment (`g Ctrl-A` in visual mode).
///
/// Finds all numbers within the selection and increments them sequentially:
/// first number += count, second += 2*count, third += 3*count, etc.
/// Processes from end to start to avoid offset invalidation.
pub fn execute_sequential_increment(ctx: &ActionContext<'_>) -> CommandResult {
    execute_sequential(ctx, true)
}

/// Sequential decrement (`g Ctrl-X` in visual mode).
///
/// Like sequential increment, but subtracts: first -= count, second -= 2*count, etc.
pub fn execute_sequential_decrement(ctx: &ActionContext<'_>) -> CommandResult {
    execute_sequential(ctx, false)
}

/// Collect all numbers within a byte range of text.
fn collect_numbers_in_range(text: &str, start: usize, end: usize) -> Vec<NumberMatch> {
    let mut numbers = Vec::new();
    let mut search_from = start;
    while search_from <= end {
        let Some(found) = find_number(text, search_from) else {
            break;
        };
        if found.start > end {
            break;
        }
        search_from = found.end;
        numbers.push(found);
    }
    numbers
}

/// Compute sequential delta for a number at position `index` (0-based).
fn sequential_delta(count: i64, index: usize, increment: bool) -> i64 {
    let multiplier = byte_delta::to_i64(index).saturating_add(1);
    let delta = count.saturating_mul(multiplier);
    if increment {
        delta
    } else {
        -delta
    }
}

/// Core sequential increment/decrement implementation.
fn execute_sequential(ctx: &ActionContext<'_>, increment: bool) -> CommandResult {
    use crate::primitives::Mode;

    let Some(selection) = ctx.selection else {
        return if increment {
            execute_increment_number(ctx)
        } else {
            execute_decrement_number(ctx)
        };
    };

    let numbers =
        collect_numbers_in_range(ctx.text, selection.start().get(), selection.end().get());
    if numbers.is_empty() {
        let effects = Effects::new()
            .clear_selection()
            .set_mode(Mode::Normal)
            .set_cursor(Offset::new(selection.start().get()));
        return CommandResult::effects_only(effects);
    }

    let count_i64 = i64::from(ctx.count);
    let mut effects = Effects::new().begin_undo();

    // Process from end to start for stable offsets
    for (i, m) in numbers.iter().enumerate().rev() {
        let new_value = m
            .value
            .saturating_add(sequential_delta(count_i64, i, increment));
        effects = effects.replace(
            Range::from_raw(m.start, m.end),
            format_number(new_value, m.base, m.digit_count),
        );
    }

    // Explicit change marks: [=first number start, ]=last number end.
    // Processing is bottom-to-top (rev) for stable offsets, so
    // sync_change_marks records the wrong order. Override explicitly.
    // Account for cumulative size changes from earlier replacements.
    // numbers is guaranteed non-empty (early return above).
    #[allow(
        clippy::indexing_slicing,
        reason = "guaranteed non-empty by early return"
    )]
    let first_number = &numbers[0];
    #[allow(
        clippy::indexing_slicing,
        reason = "guaranteed non-empty by early return"
    )]
    let last_number = &numbers[numbers.len() - 1];
    let last_new_value =
        last_number
            .value
            .saturating_add(sequential_delta(count_i64, numbers.len() - 1, increment));
    let last_new_text = format_number(last_new_value, last_number.base, last_number.digit_count);
    let mark_start = Offset::new(first_number.start);
    let cumulative_shift: isize = numbers
        .iter()
        .enumerate()
        .take(numbers.len() - 1)
        .map(|(i, m)| {
            let nv = m
                .value
                .saturating_add(sequential_delta(count_i64, i, increment));
            let nt = format_number(nv, m.base, m.digit_count);
            byte_delta::delta(nt.len(), m.end - m.start)
        })
        .sum();
    let last_new_start = last_number.start.saturating_add_signed(cumulative_shift);
    let mark_end = Offset::new(last_new_start + last_new_text.len().saturating_sub(1));
    // Neovim's op_addsub calls changed_lines(start.lnum, 0, ...) after
    // all changes, setting mark '.' to (start.lnum, col=0) = line start
    // of the first selected line. Ref: ops.c:2380
    let sel_start = selection.start().get();
    let mark_dot = Offset::new(crate::commands::helpers::line_start_for_offset(
        ctx.text, sel_start,
    ));
    effects = effects
        .set_mark(crate::primitives::MarkName::CHANGE_START, mark_start, None)
        .set_mark(crate::primitives::MarkName::CHANGE_END, mark_end, None)
        .set_mark(crate::primitives::MarkName::LAST_CHANGE, mark_dot, None);

    // Cursor at selection start (Neovim behavior), exit visual mode
    let cursor = Offset::new(sel_start);
    CommandResult::effects_only(
        effects
            .set_cursor(cursor)
            .end_undo()
            .clear_selection()
            .set_mode(Mode::Normal),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    fn make_ctx(text: &str, cursor: usize, count: u32) -> ActionContext<'_> {
        ActionContext::from_text_and_cursor(
            text,
            Offset::new(cursor),
            NonZeroU32::new(count).unwrap_or(NonZeroU32::MIN),
        )
    }

    #[test]
    fn test_increment_decimal() {
        let ctx = make_ctx("foo 123 bar", 4, 1);
        let result = execute_increment_number(&ctx);
        // begin_undo + replace + set_cursor + end_undo = 4 effects
        assert_eq!(result.effects.len(), 4);
        // "123" -> "124"
    }

    #[test]
    fn test_decrement_decimal() {
        let ctx = make_ctx("foo 123 bar", 4, 1);
        let result = execute_decrement_number(&ctx);
        // begin_undo + replace + set_cursor + end_undo = 4 effects
        assert_eq!(result.effects.len(), 4);
        // "123" -> "122"
    }

    #[test]
    fn test_increment_hex() {
        let ctx = make_ctx("0xff", 0, 1);
        let result = execute_increment_number(&ctx);
        // begin_undo + replace + set_cursor + end_undo = 4 effects
        assert_eq!(result.effects.len(), 4);
        // "0xff" -> "0x100"
    }

    #[test]
    fn test_increment_negative() {
        let ctx = make_ctx("-5", 0, 1);
        let result = execute_increment_number(&ctx);
        // begin_undo + replace + set_cursor + end_undo = 4 effects
        assert_eq!(result.effects.len(), 4);
        // "-5" -> "-4"
    }

    #[test]
    fn test_increment_with_count() {
        let ctx = make_ctx("10", 0, 5);
        let result = execute_increment_number(&ctx);
        // begin_undo + replace + set_cursor + end_undo = 4 effects
        assert_eq!(result.effects.len(), 4);
        // "10" -> "15"
    }

    #[test]
    fn test_no_number() {
        let ctx = make_ctx("foo bar", 0, 1);
        let result = execute_increment_number(&ctx);
        assert!(result.effects.is_empty());
    }

    // ── find_boolean tests ──────────────────────────────────────────────

    #[test]
    fn find_boolean_at_cursor() {
        let (start, end, word) = find_boolean("true", 0).unwrap();
        assert_eq!(start, 0);
        assert_eq!(end, 4);
        assert_eq!(word, "true");
    }

    #[test]
    fn find_boolean_after_cursor() {
        let (start, end, word) = find_boolean("let x = false;", 0).unwrap();
        assert_eq!(start, 8);
        assert_eq!(end, 13);
        assert_eq!(word, "false");
    }

    #[test]
    fn find_boolean_returns_none_for_non_boolean() {
        assert!(find_boolean("hello world", 0).is_none());
    }

    #[test]
    fn find_boolean_respects_word_boundary() {
        // "falsehood" should NOT match "false" because 'h' follows
        assert!(find_boolean("falsehood", 0).is_none());
    }

    #[test]
    fn find_boolean_respects_word_boundary_before() {
        // "xtrue" should NOT match "true" because 'x' precedes
        assert!(find_boolean("xtrue", 0).is_none());
    }

    #[test]
    fn find_boolean_in_middle_of_line() {
        let (start, end, word) = find_boolean("val = Yes;", 4).unwrap();
        assert_eq!(start, 6);
        assert_eq!(end, 9);
        assert_eq!(word, "Yes");
    }

    #[test]
    fn find_boolean_does_not_cross_newline() {
        // "true" is on next line; cursor is on first line
        assert!(find_boolean("hello\ntrue", 0).is_none());
    }

    // ── toggle_boolean tests ────────────────────────────────────────────

    #[test]
    fn toggle_true_to_false() {
        assert_eq!(toggle_boolean("true").unwrap().as_str(), "false");
    }

    #[test]
    fn toggle_false_to_true() {
        assert_eq!(toggle_boolean("false").unwrap().as_str(), "true");
    }

    #[test]
    fn toggle_title_case() {
        assert_eq!(toggle_boolean("True").unwrap().as_str(), "False");
        assert_eq!(toggle_boolean("False").unwrap().as_str(), "True");
    }

    #[test]
    fn toggle_upper_case() {
        assert_eq!(toggle_boolean("TRUE").unwrap().as_str(), "FALSE");
        assert_eq!(toggle_boolean("FALSE").unwrap().as_str(), "TRUE");
    }

    #[test]
    fn toggle_yes_no() {
        assert_eq!(toggle_boolean("yes").unwrap().as_str(), "no");
        assert_eq!(toggle_boolean("no").unwrap().as_str(), "yes");
        assert_eq!(toggle_boolean("No").unwrap().as_str(), "Yes");
        assert_eq!(toggle_boolean("YES").unwrap().as_str(), "NO");
        assert_eq!(toggle_boolean("NO").unwrap().as_str(), "YES");
    }

    #[test]
    fn toggle_on_off() {
        assert_eq!(toggle_boolean("on").unwrap().as_str(), "off");
        assert_eq!(toggle_boolean("off").unwrap().as_str(), "on");
        assert_eq!(toggle_boolean("On").unwrap().as_str(), "Off");
        assert_eq!(toggle_boolean("Off").unwrap().as_str(), "On");
        assert_eq!(toggle_boolean("ON").unwrap().as_str(), "OFF");
        assert_eq!(toggle_boolean("OFF").unwrap().as_str(), "ON");
    }

    #[test]
    fn toggle_unknown_returns_none() {
        assert!(toggle_boolean("hello").is_none());
    }

    // ── Integration: execute_increment/decrement with booleans ──────────

    #[test]
    fn increment_on_true_toggles_to_false() {
        let ctx = make_ctx("true", 0, 1);
        let result = execute_increment_number(&ctx);
        // Should have 4 effects: begin_undo + replace + set_cursor + end_undo
        assert_eq!(result.effects.len(), 4);
        // Verify the replace effect has "false"
        let has_replace = result.effects.iter().any(|e| {
            matches!(e, crate::effects::Effect::Replace { text, .. } if text.as_str() == "false")
        });
        assert!(has_replace, "should replace 'true' with 'false'");
    }

    #[test]
    fn decrement_on_true_also_toggles_to_false() {
        let ctx = make_ctx("true", 0, 1);
        let result = execute_decrement_number(&ctx);
        // Symmetric: Ctrl-X also toggles
        assert_eq!(result.effects.len(), 4);
        let has_replace = result.effects.iter().any(|e| {
            matches!(e, crate::effects::Effect::Replace { text, .. } if text.as_str() == "false")
        });
        assert!(has_replace, "Ctrl-X should also toggle 'true' to 'false'");
    }

    #[test]
    fn increment_on_false_toggles_to_true() {
        let ctx = make_ctx("false", 0, 1);
        let result = execute_increment_number(&ctx);
        assert_eq!(result.effects.len(), 4);
        let has_replace = result.effects.iter().any(|e| {
            matches!(e, crate::effects::Effect::Replace { text, .. } if text.as_str() == "true")
        });
        assert!(has_replace, "should replace 'false' with 'true'");
    }

    #[test]
    fn increment_number_fallback_when_no_boolean() {
        // "42" is not a boolean — should fall back to number increment
        let ctx = make_ctx("42", 0, 1);
        let result = execute_increment_number(&ctx);
        assert_eq!(result.effects.len(), 4);
        // "42" → "43"
        let has_replace = result.effects.iter().any(
            |e| matches!(e, crate::effects::Effect::Replace { text, .. } if text.as_str() == "43"),
        );
        assert!(has_replace, "should increment number when no boolean found");
    }

    #[test]
    fn increment_no_boolean_no_number_empty() {
        let ctx = make_ctx("hello world", 0, 1);
        let result = execute_increment_number(&ctx);
        assert!(
            result.effects.is_empty(),
            "no boolean and no number → no effects"
        );
    }

    #[test]
    fn toggle_boolean_after_cursor_on_same_line() {
        // Cursor at position 0, "false" starts at position 8
        let ctx = make_ctx("let x = false;", 0, 1);
        let result = execute_increment_number(&ctx);
        assert_eq!(result.effects.len(), 4);
        let has_replace = result.effects.iter().any(|e| {
            matches!(e, crate::effects::Effect::Replace { text, .. } if text.as_str() == "true")
        });
        assert!(has_replace, "should find and toggle 'false' after cursor");
    }

    #[test]
    fn toggle_boolean_case_preservation_upper() {
        let ctx = make_ctx("ON", 0, 1);
        let result = execute_increment_number(&ctx);
        assert_eq!(result.effects.len(), 4);
        let has_replace = result.effects.iter().any(
            |e| matches!(e, crate::effects::Effect::Replace { text, .. } if text.as_str() == "OFF"),
        );
        assert!(has_replace, "should toggle 'ON' to 'OFF' preserving case");
    }

    #[test]
    fn toggle_boolean_prefers_boolean_over_number_at_same_position() {
        // "true 42" — boolean comes first, should toggle it
        let ctx = make_ctx("true 42", 0, 1);
        let result = execute_increment_number(&ctx);
        let has_bool_replace = result.effects.iter().any(|e| {
            matches!(e, crate::effects::Effect::Replace { text, .. } if text.as_str() == "false")
        });
        assert!(has_bool_replace, "boolean should be preferred over number");
    }
}
