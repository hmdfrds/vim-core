//! Display ex commands (`:print`, `:list`, `:number`, `:z`).
//!
//! Read-only commands that display lines without modifying the buffer.

use std::fmt::Write;

use compact_str::CompactString;

use super::range::resolve_range;
use super::types::{ExContext, ExResult};
use crate::effects::Effects;
use crate::grammar::types::{ExRange, ZWindowStyle};

/// Display lines (`:print`, `:number`, `:list`, `:#`).
///
/// - `number`: prepend 1-indexed line numbers (right-aligned, 6 columns).
/// - `list`: show control characters as `^X`, tabs as `^I`, and `$` at EOL.
///
/// Vim allows combining both (`:nu l` or `:#l`).
/// Sets cursor to the last displayed line per Vim semantics.
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if range is invalid.
pub fn print_lines(range: &ExRange, number: bool, list: bool, ctx: &ExContext) -> ExResult {
    let resolved = resolve_range(range, ctx)?;
    let mut output = String::new();

    for line_idx in resolved.start()..=resolved.end() {
        if let Some(line_text) = ctx.line_text(line_idx) {
            if number {
                let _ = write!(output, "{:>6}  ", line_idx + 1);
            }
            if list {
                for ch in line_text.chars() {
                    match ch {
                        '\t' => output.push_str("^I"),
                        c if c.is_control() => {
                            output.push('^');
                            output.push((c as u8 + b'@') as char);
                        }
                        c => output.push(c),
                    }
                }
                output.push('$');
            } else {
                output.push_str(line_text);
            }
            output.push('\n');
        }
    }

    let mut effects = Effects::new().show_message(CompactString::from(output));
    if let Some(last_offset) = ctx.line_start_offset(resolved.end()) {
        effects = effects.set_cursor(last_offset);
    }
    Ok(effects)
}

/// Window display (`:z[+-=.^#] [count]`).
///
/// Shows a screenful of lines around the target line. The style modifier
/// controls which lines are shown relative to the target:
///
/// - `Below` / `+`: target through target+count
/// - `Above` / `-`: target-count through target
/// - `Centered` / `.`: target centered in window
/// - `Highlighted` / `=`: centered with target surrounded by dashes
/// - `PrevWindow` / `^`: the window ending just before target
/// - `Numbered` / `#`: like `Below` but with line numbers
///
/// # Errors
///
/// Returns `VimError::InvalidRange` if the target line is invalid.
pub fn z_window(
    range: &ExRange,
    style: ZWindowStyle,
    count: Option<u32>,
    ctx: &ExContext,
) -> ExResult {
    let target = resolve_range(range, ctx)?.start();
    let window_size = count.unwrap_or(20) as usize;
    let last_line = ctx.total_lines.saturating_sub(1);

    let (start, end) = match style {
        ZWindowStyle::Below | ZWindowStyle::Numbered => {
            (target, (target + window_size).min(last_line))
        }
        ZWindowStyle::Above => (target.saturating_sub(window_size), target),
        ZWindowStyle::Centered | ZWindowStyle::Highlighted => {
            let half = window_size / 2;
            (target.saturating_sub(half), (target + half).min(last_line))
        }
        ZWindowStyle::PrevWindow => (target.saturating_sub(window_size), target.saturating_sub(1)),
    };

    let show_numbers = matches!(style, ZWindowStyle::Numbered | ZWindowStyle::Highlighted);
    let mut output = String::new();

    for line_idx in start..=end {
        if let Some(line_text) = ctx.line_text(line_idx) {
            if show_numbers {
                let _ = write!(output, "{:>6}  ", line_idx + 1);
            }
            if style == ZWindowStyle::Highlighted && line_idx == target {
                // Highlight target line with dashes above and below.
                let _ = writeln!(output, "{:-<40}", "");
                if show_numbers {
                    let _ = write!(output, "{:>6}  ", line_idx + 1);
                }
                output.push_str(line_text);
                output.push('\n');
                let _ = writeln!(output, "{:-<40}", "");
            } else {
                output.push_str(line_text);
                output.push('\n');
            }
        }
    }

    let cursor_line = match style {
        ZWindowStyle::PrevWindow => start,
        _ => end,
    };
    let mut effects = Effects::new().show_message(CompactString::from(output));
    if let Some(offset) = ctx.line_start_offset(cursor_line.min(last_line)) {
        effects = effects.set_cursor(offset);
    }
    Ok(effects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Offset;

    fn make_ctx(text: &str) -> ExContext<'_> {
        ExContext::new(text, 0)
    }

    // ── :print / :number / :list ──────────────────────────────────────

    #[test]
    fn print_lines_plain() {
        let ctx = make_ctx("aaa\nbbb\nccc");
        let effects = print_lines(&ExRange::entire_file(), false, false, &ctx).unwrap();
        let msg = extract_message(&effects);
        assert!(msg.contains("aaa"));
        assert!(msg.contains("bbb"));
        assert!(msg.contains("ccc"));
        // No line numbers in plain mode.
        assert!(!msg.contains("     1"));
    }

    #[test]
    fn print_lines_with_numbers() {
        let ctx = make_ctx("aaa\nbbb\nccc");
        let effects = print_lines(&ExRange::entire_file(), true, false, &ctx).unwrap();
        let msg = extract_message(&effects);
        assert!(msg.contains("     1  aaa"));
        assert!(msg.contains("     2  bbb"));
        assert!(msg.contains("     3  ccc"));
    }

    #[test]
    fn print_lines_list_shows_control_chars() {
        let ctx = make_ctx("a\tb");
        let effects = print_lines(&ExRange::current_line(), false, true, &ctx).unwrap();
        let msg = extract_message(&effects);
        assert!(msg.contains("^I"), "tabs should be shown as ^I: {msg:?}");
        assert!(msg.contains('$'), "EOL marker should be present: {msg:?}");
    }

    #[test]
    fn print_lines_number_and_list_combined() {
        let ctx = make_ctx("x\ty");
        let effects = print_lines(&ExRange::current_line(), true, true, &ctx).unwrap();
        let msg = extract_message(&effects);
        assert!(msg.contains("     1"), "line numbers expected: {msg:?}");
        assert!(msg.contains("^I"), "tabs as ^I expected: {msg:?}");
        assert!(msg.contains('$'), "EOL marker expected: {msg:?}");
    }

    #[test]
    fn print_lines_sets_cursor_to_last_line() {
        let ctx = make_ctx("aaa\nbbb\nccc");
        let effects = print_lines(&ExRange::entire_file(), false, false, &ctx).unwrap();
        let has_cursor = effects.iter().any(|e| {
            matches!(e, crate::effects::Effect::SetCursor { offset } if *offset == Offset::new(8))
        });
        assert!(has_cursor, "cursor should be set to start of last line");
    }

    // ── :z window ─────────────────────────────────────────────────────

    #[test]
    fn z_window_below_shows_correct_range() {
        // 10 lines, start at line 0, show 3 lines below
        let text = (0..10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let ctx = make_ctx(&text);
        let effects =
            z_window(&ExRange::current_line(), ZWindowStyle::Below, Some(3), &ctx).unwrap();
        let msg = extract_message(&effects);
        assert!(msg.contains("line0"), "should start at target: {msg:?}");
        assert!(msg.contains("line3"), "should include +3: {msg:?}");
        assert!(!msg.contains("line4"), "should not include +4: {msg:?}");
    }

    #[test]
    fn z_window_above_shows_correct_range() {
        let text = (0..10)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut ctx = make_ctx(&text);
        ctx.cursor_line = 5;
        let effects =
            z_window(&ExRange::current_line(), ZWindowStyle::Above, Some(3), &ctx).unwrap();
        let msg = extract_message(&effects);
        assert!(msg.contains("line2"), "should start at target-3: {msg:?}");
        assert!(msg.contains("line5"), "should include target: {msg:?}");
        assert!(
            !msg.contains("line6"),
            "should not include target+1: {msg:?}"
        );
    }

    #[test]
    fn z_window_numbered_shows_line_numbers() {
        let text = "aaa\nbbb\nccc";
        let ctx = make_ctx(text);
        let effects = z_window(
            &ExRange::current_line(),
            ZWindowStyle::Numbered,
            Some(2),
            &ctx,
        )
        .unwrap();
        let msg = extract_message(&effects);
        assert!(msg.contains("     1"), "should show line numbers: {msg:?}");
    }

    #[test]
    fn z_window_highlighted_shows_dashes() {
        let text = "aaa\nbbb\nccc";
        let mut ctx = make_ctx(text);
        ctx.cursor_line = 1;
        let effects = z_window(
            &ExRange::current_line(),
            ZWindowStyle::Highlighted,
            Some(2),
            &ctx,
        )
        .unwrap();
        let msg = extract_message(&effects);
        assert!(
            msg.contains("---"),
            "highlighted should contain dashes: {msg:?}"
        );
    }

    /// Extract the text message from effects.
    fn extract_message(effects: &Effects) -> String {
        effects
            .iter()
            .filter_map(|e| match e {
                crate::effects::Effect::ShowInfo {
                    info: crate::effects::InfoMessage::Text(text),
                    ..
                } => Some(text.as_str().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}
