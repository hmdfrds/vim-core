//! Sort command (`:sort [options]`).
//!
//! Sort lines in a range with various options.

use super::range::resolve_range;
use super::types::{ExContext, ExResult};
use crate::effects::Effects;
use crate::grammar::types::{ExRange, SortOptions};
use crate::primitives::{MarkName, Offset, Range};

/// Execute sort command.
///
/// Sorts lines in the specified range according to options.
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if range is invalid.
pub fn sort(range: &ExRange, options: &SortOptions, ctx: &ExContext) -> ExResult {
    let resolved = resolve_range(range, ctx)?;
    let (start_offset, end_offset) = ctx
        .lines_range(resolved.start(), resolved.end())
        .ok_or(crate::errors::VimError::InvalidRange)?;

    // Collect lines
    let mut lines: Vec<&str> = Vec::new();
    for line_idx in resolved.start()..=resolved.end() {
        if let Some(line_text) = ctx.line_text(line_idx) {
            lines.push(line_text);
        }
    }

    // Compile pattern regex if present (for sort key extraction).
    let pattern_re = options
        .pattern()
        .and_then(|p| crate::regex::VimRegex::new(p).ok());

    // Sort based on options (stable sort — Vim preserves relative order of equal elements).
    // When a pattern is given, the sort key is the text AFTER the pattern match.
    #[allow(
        clippy::stable_sort_primitive,
        reason = "Vim :sort preserves relative order of equal elements"
    )]
    if let Some(ref re) = pattern_re {
        // Sort by text after pattern match. If no match, use entire line.
        let mut cache = re.create_cache();
        lines.sort_by_cached_key(|s| {
            let ctx = crate::regex::MatchContext::simple(s);
            if let Ok(Some(m)) = re.find_with_cache(&mut cache, &ctx) {
                s[m.range.end..].to_string()
            } else {
                s.to_string()
            }
        });
    } else if options.numeric() {
        lines.sort_by_key(|s| extract_leading_number(s));
    } else if options.ignore_case() {
        lines.sort_by_cached_key(|s| s.to_lowercase());
    } else {
        lines.sort();
    }

    // Reverse if needed
    if options.reverse() {
        lines.reverse();
    }

    // Remove duplicates if unique
    if options.unique() {
        lines.dedup_by(|a, b| {
            if options.ignore_case() {
                a.to_lowercase() == b.to_lowercase()
            } else {
                a == b
            }
        });
    }

    // Build replacement text
    let mut new_text = String::new();
    for (i, line) in lines.iter().enumerate() {
        new_text.push_str(line);
        // Add newline unless it's the last line AND it was the last line of file
        if i < lines.len() - 1 || resolved.end() + 1 < ctx.total_lines {
            new_text.push('\n');
        }
    }

    let line_count = resolved.line_count();
    let effects = Effects::new()
        .begin_undo()
        .replace(Range::new(start_offset, end_offset), &new_text)
        .set_cursor(start_offset)
        .show_message(format!("{line_count} lines sorted"))
        .end_undo();

    Ok(effects)
}

/// Extract leading number from a string for numeric sort.
fn extract_leading_number(s: &str) -> i64 {
    let trimmed = s.trim_start();
    let bytes = trimmed.as_bytes();
    if bytes.is_empty() {
        return 0;
    }
    let (neg, start) = match bytes.first() {
        Some(b'-') => (true, 1),
        Some(b'+') => (false, 1),
        _ => (false, 0),
    };
    let mut val: i64 = 0;
    for &b in bytes.get(start..).unwrap_or(&[]) {
        if b.is_ascii_digit() {
            val = val.saturating_mul(10).saturating_add(i64::from(b - b'0'));
        } else {
            break;
        }
    }
    if neg {
        val.saturating_neg()
    } else {
        val
    }
}

/// Compute cursor position at start of last line in inserted text.
fn last_inserted_line_offset(insert_text: &str, base_cursor: usize) -> usize {
    let content_start = usize::from(insert_text.starts_with('\n'));
    let content = &insert_text[content_start..];
    let content_trimmed = content.trim_end_matches('\n');
    let last_line_offset = content_trimmed.rfind('\n').map_or(0, |p| p + 1);
    base_cursor + last_line_offset
}

/// Move lines to a target address (`:m`).
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if range is invalid.
pub fn move_lines(
    range: &ExRange,
    target_line: Option<usize>, // 0-indexed destination, None = before first line
    ctx: &ExContext,
) -> ExResult {
    let resolved = resolve_range(range, ctx)?;
    let (start_offset, end_offset) = ctx
        .lines_range(resolved.start(), resolved.end())
        .ok_or(crate::errors::VimError::InvalidRange)?;

    // Get text to move
    let raw_text = Range::new(start_offset, end_offset).slice(ctx.text);

    // Calculate target offset (after target line)
    // None means before first line (`:0m` semantics) => insert at offset 0
    let (target_offset, target_line_val) = match target_line {
        None => (Offset::new(0), 0usize),
        Some(tl) => {
            let off = if tl >= ctx.total_lines {
                Offset::new(ctx.text.len())
            } else {
                ctx.line_start_offset(tl + 1)
                    .unwrap_or_else(|| Offset::new(ctx.text.len()))
            };
            (off, tl)
        }
    };

    // Adjust target if moving within the range being moved
    if target_line.is_some()
        && target_line_val >= resolved.start()
        && target_line_val <= resolved.end()
    {
        // Vim E134: Cannot move a range of lines into itself
        return Err(crate::errors::VimError::MoveIntoItself);
    }

    let moving_down = target_line.is_some() && target_line_val > resolved.end();
    let source_len = start_offset.distance(end_offset);

    // Build insert text with proper newline handling.
    // We compute offsets for the insert-first-then-delete order, matching
    // Neovim's do_move() which inserts at the destination before deleting
    // from the source.  This order also produces changelist entries in the
    // correct sequence (insert position first, delete position second).
    let mut insert_text = String::new();

    // Insert position: the raw target_offset (before any delete).
    // For insert-at-end-of-document, check current text (before any edit).
    let inserting_at_end = target_offset.get() >= ctx.text.len();
    if inserting_at_end && !ctx.text.is_empty() {
        // If text doesn't end with newline, we need a separator
        if !ctx.text.ends_with('\n') {
            insert_text.push('\n');
        }
    }
    insert_text.push_str(raw_text);
    if !insert_text.ends_with('\n') {
        insert_text.push('\n');
    }

    // After the insert, the source range shifts if the insert was before it.
    let (delete_start, delete_end) = if target_offset.get() <= start_offset.get() {
        // Insert was before source: source shifts by insert_text.len()
        let shift = insert_text.len();
        (
            Offset::new(start_offset.get() + shift),
            Offset::new(end_offset.get() + shift),
        )
    } else {
        // Insert was after source: source range unchanged
        (start_offset, end_offset)
    };

    // Compute the final cursor position and marks in the post-edit text.
    // After insert-then-delete, the final text is the same as delete-then-insert.
    // Use the adjusted_target (post-both-edits) for cursor/mark computation.
    let adjusted_target = if moving_down {
        target_offset.saturating_sub_raw(source_len)
    } else {
        target_offset
    };

    // Check if the final insert position is at end of post-edit text.
    let text_after_both_len = ctx.text.len(); // insert + delete = same total
    let final_at_end = adjusted_target.get() >= text_after_both_len - source_len;

    let base_cursor = if final_at_end && insert_text.starts_with('\n') {
        adjusted_target.next().get()
    } else {
        adjusted_target.get()
    };
    let cursor_pos = last_inserted_line_offset(&insert_text, base_cursor);

    // Neovim marks after :move:
    //   '[' = start of first moved line in final position
    //   ']' = start of last moved line in final position
    //   '.' = min of source start and insert position (first affected byte)
    let mark_start = Offset::new(base_cursor);
    let mark_end = Offset::new(cursor_pos);
    let mark_dot = Offset::new(start_offset.get().min(adjusted_target.get()));

    let line_count = resolved.line_count();
    // Neovim's do_move() calls u_save() twice (once before the insert at
    // the destination, once before the delete at the source), producing two
    // changelist entries.  We use two undo groups so that sync_effects_with_text
    // resets changelist coalescing at the second BeginUndoGroup, allowing each
    // mutation to push its own changelist entry.  Insert first matches Neovim's
    // operation order, yielding changelist entries [insert_pos, delete_pos].
    let effects = Effects::new()
        .begin_undo()
        .insert(target_offset, insert_text)
        .end_undo()
        .begin_undo()
        .delete(Range::new(delete_start, delete_end))
        .set_mark(MarkName::CHANGE_START, mark_start, None)
        .set_mark(MarkName::CHANGE_END, mark_end, None)
        .set_mark(MarkName::LAST_CHANGE, mark_dot, None)
        .set_cursor(Offset::new(cursor_pos))
        .show_message(format!("{line_count} lines moved"))
        .end_undo();

    Ok(effects)
}

/// Copy lines to a target address (`:t` or `:copy`).
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if range is invalid.
pub fn copy_lines(
    range: &ExRange,
    target_line: Option<usize>, // 0-indexed destination, None = before first line
    ctx: &ExContext,
) -> ExResult {
    let resolved = resolve_range(range, ctx)?;
    let (start_offset, end_offset) = ctx
        .lines_range(resolved.start(), resolved.end())
        .ok_or(crate::errors::VimError::InvalidRange)?;

    // Get text to copy
    let raw_text = Range::new(start_offset, end_offset).slice(ctx.text);

    // Calculate target offset (after target line, or at offset 0 for "before first")
    let target_offset = match target_line {
        None => Offset::new(0), // `:co 0` — before first line
        Some(line) if line >= ctx.total_lines => Offset::new(ctx.text.len()),
        Some(line) => ctx
            .line_start_offset(line + 1)
            .unwrap_or_else(|| Offset::new(ctx.text.len())),
    };

    // Build the text to insert, ensuring proper line separation:
    // - If inserting at end of document and last char isn't newline, prepend \n
    // - Ensure copied text always ends with newline (linewise)
    let mut insert_text = String::new();
    if target_offset.get() == ctx.text.len() && !ctx.text.is_empty() && !ctx.text.ends_with('\n') {
        insert_text.push('\n');
    }
    insert_text.push_str(raw_text);
    // Ensure trailing newline for linewise copy (unless at very end of document)
    if !insert_text.ends_with('\n') {
        insert_text.push('\n');
    }

    // Vim places cursor at the start of the LAST copied line
    let base_cursor = if target_offset.get() == ctx.text.len()
        && !ctx.text.is_empty()
        && !ctx.text.ends_with('\n')
    {
        target_offset.next().get() // after the prepended \n
    } else {
        target_offset.get()
    };
    let cursor_pos = last_inserted_line_offset(&insert_text, base_cursor);

    // Neovim marks after :copy:
    //   '[' = start of first copied line (content, after any prepended \n)
    //   ']' = start of last copied line
    //   '.' = same as '['
    let mark_start = Offset::new(base_cursor);
    let mark_end = Offset::new(cursor_pos);

    let line_count = resolved.line_count();
    let effects = Effects::new()
        .begin_undo()
        .insert(target_offset, insert_text)
        .set_mark(MarkName::CHANGE_START, mark_start, None)
        .set_mark(MarkName::CHANGE_END, mark_end, None)
        .set_mark(MarkName::LAST_CHANGE, mark_start, None)
        .set_cursor(Offset::new(cursor_pos))
        .show_message(format!("{line_count} lines copied"))
        .end_undo();

    Ok(effects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::Effect;

    fn ctx() -> ExContext<'static> {
        ExContext::new("cherry\napple\nbanana", 0)
    }

    #[test]
    fn test_sort_alphabetic() {
        let range = ExRange::entire_file();
        let result = sort(&range, &SortOptions::default(), &ctx()).unwrap();

        assert!(result.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn test_sort_reverse() {
        let range = ExRange::entire_file();
        let options = SortOptions::parse("r");
        let result = sort(&range, &options, &ctx()).unwrap();

        assert!(result.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn test_sort_numeric() {
        let ctx = ExContext::new("10 items\n2 items\n100 items", 0);
        let range = ExRange::entire_file();
        let options = SortOptions::parse("n");
        let result = sort(&range, &options, &ctx).unwrap();

        assert!(result.iter().any(|e| matches!(e, Effect::Replace { .. })));
    }

    #[test]
    fn test_copy_lines() {
        let range = ExRange::single_line(1);
        let result = copy_lines(&range, Some(2), &ctx()).unwrap();

        assert!(result.iter().any(|e| matches!(e, Effect::Insert { .. })));
    }

    // ── move_lines tests ─────────────────────────────────────────────────

    #[test]
    fn move_lines_down() {
        // Move line 1 ("cherry") after line 3 ("banana").
        // "cherry\napple\nbanana" → delete "cherry\n", insert after "banana".
        let range = ExRange::single_line(1);
        let result = move_lines(&range, Some(2), &ctx()).unwrap();

        // Should have Delete + Insert effects (inside undo group).
        assert!(result.iter().any(|e| matches!(e, Effect::Delete { .. })));
        assert!(result.iter().any(|e| matches!(e, Effect::Insert { .. })));
    }

    #[test]
    fn move_lines_up() {
        // Move line 3 ("banana") before line 1 (target=0 means "before line 1").
        // "cherry\napple\nbanana" → delete "banana", insert at top.
        let ctx = ExContext::new("cherry\napple\nbanana\n", 0);
        let range = ExRange::single_line(3);
        let result = move_lines(&range, None, &ctx).unwrap();

        assert!(result.iter().any(|e| matches!(e, Effect::Delete { .. })));
        assert!(result.iter().any(|e| matches!(e, Effect::Insert { .. })));
    }

    #[test]
    fn move_lines_within_range_returns_error() {
        // Moving line 2 after line 2 (within itself) → E134 error.
        let range = ExRange::single_line(2);
        let result = move_lines(&range, Some(1), &ctx());

        assert!(matches!(
            result,
            Err(crate::errors::VimError::MoveIntoItself)
        ));
    }

    #[test]
    fn move_lines_range_within_itself_returns_error() {
        // Moving lines 1-3 to target 2 (inside the range) → E134 error.
        let range = ExRange::lines(1, 3);
        let result = move_lines(&range, Some(1), &ctx());

        assert!(matches!(
            result,
            Err(crate::errors::VimError::MoveIntoItself)
        ));
    }

    // ── extract_leading_number ───────────────────────────────────────────

    #[test]
    fn extract_leading_number_empty_string() {
        assert_eq!(extract_leading_number(""), 0);
    }

    #[test]
    fn extract_leading_number_whitespace_only() {
        assert_eq!(extract_leading_number("   "), 0);
    }

    #[test]
    fn extract_leading_number_bare_plus() {
        assert_eq!(extract_leading_number("+"), 0);
    }

    #[test]
    fn extract_leading_number_bare_minus() {
        assert_eq!(extract_leading_number("-"), 0);
    }

    #[test]
    fn extract_leading_number_positive() {
        assert_eq!(extract_leading_number("42 apples"), 42);
    }

    #[test]
    fn extract_leading_number_negative() {
        assert_eq!(extract_leading_number("-7 degrees"), -7);
    }

    #[test]
    fn extract_leading_number_explicit_plus() {
        assert_eq!(extract_leading_number("+99 items"), 99);
    }

    #[test]
    fn extract_leading_number_leading_whitespace() {
        assert_eq!(extract_leading_number("  123xyz"), 123);
    }

    #[test]
    fn extract_leading_number_non_digit_prefix() {
        assert_eq!(extract_leading_number("abc123"), 0);
    }

    #[test]
    fn extract_leading_number_zero() {
        assert_eq!(extract_leading_number("0"), 0);
    }

    #[test]
    fn extract_leading_number_negative_zero() {
        assert_eq!(extract_leading_number("-0"), 0);
    }

    #[test]
    fn extract_leading_number_overflow_saturates() {
        // i64::MAX = 9_223_372_036_854_775_807 — use a string far exceeding that
        assert_eq!(
            extract_leading_number("99999999999999999999999999999"),
            i64::MAX
        );
    }

    #[test]
    fn extract_leading_number_negative_overflow_saturates() {
        // Positive accumulation saturates to i64::MAX, then saturating_neg()
        // yields -i64::MAX (= i64::MIN + 1), not i64::MIN.
        assert_eq!(
            extract_leading_number("-99999999999999999999999999999"),
            -i64::MAX
        );
    }
}
