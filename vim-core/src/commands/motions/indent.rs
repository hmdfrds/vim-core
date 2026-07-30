//! Indent-based navigation motions: `[i`, `]i`, `[-`, `]-`, `[+`, `]+`
//!
//! Navigate between lines by comparing indentation levels.
//! Bound to unimpaired-style bracket mappings.
//!
//! # Algorithm
//!
//! 1. Measure current line's indentation in columns (tabs → tabstop boundaries)
//! 2. Scan lines up/down, skipping blank lines
//! 3. Find first non-blank line matching the indent comparison
//! 4. Position cursor on first non-blank character of target line
//!
//! # Architecture
//!
//! **ALLOWED imports**: primitives, grammar, state, effects, document
//! **FORBIDDEN imports**: operators/, mode/, execution/

use super::types::{MotionContext, MotionResult};
use crate::commands::helpers::{
    first_non_blank_in_line, is_blank_line, line_content, line_count, line_end, line_of, line_start,
};
use crate::primitives::Offset;

// ═══════════════════════════════════════════════════════════════════════════════
// Public API — one function per motion, delegating to shared scanner
// ═══════════════════════════════════════════════════════════════════════════════

/// `[i` — Previous line with same indentation level.
pub fn prev_same_indent(ctx: &MotionContext<'_>) -> MotionResult {
    indent_scan(ctx, ScanDir::Up, IndentCmp::Same)
}

/// `]i` — Next line with same indentation level.
pub fn next_same_indent(ctx: &MotionContext<'_>) -> MotionResult {
    indent_scan(ctx, ScanDir::Down, IndentCmp::Same)
}

/// `[-` — Previous line with lesser indentation (parent scope).
pub fn prev_lesser_indent(ctx: &MotionContext<'_>) -> MotionResult {
    indent_scan(ctx, ScanDir::Up, IndentCmp::Lesser)
}

/// `]-` — Next line with lesser indentation (parent scope).
pub fn next_lesser_indent(ctx: &MotionContext<'_>) -> MotionResult {
    indent_scan(ctx, ScanDir::Down, IndentCmp::Lesser)
}

/// `[+` — Previous line with greater indentation (child scope).
pub fn prev_greater_indent(ctx: &MotionContext<'_>) -> MotionResult {
    indent_scan(ctx, ScanDir::Up, IndentCmp::Greater)
}

/// `]+` — Next line with greater indentation (child scope).
pub fn next_greater_indent(ctx: &MotionContext<'_>) -> MotionResult {
    indent_scan(ctx, ScanDir::Down, IndentCmp::Greater)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Internal — shared scanner
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Clone, Copy)]
enum ScanDir {
    Up,
    Down,
}

#[derive(Clone, Copy)]
enum IndentCmp {
    Same,
    Lesser,
    Greater,
}

/// Shared indent navigation scanner.
///
/// Scans lines in `direction` from cursor, skipping blank lines.
/// Finds the `count`-th line whose indentation satisfies `comparison`
/// relative to the current line's indentation.
fn indent_scan(ctx: &MotionContext<'_>, direction: ScanDir, comparison: IndentCmp) -> MotionResult {
    let text = ctx.text;
    if text.is_empty() {
        return MotionResult::Error;
    }

    let tabstop = ctx.options.tabstop();
    let current_line = line_of(text, ctx.cursor.get());
    let total_lines = line_count(text);
    let reference_indent = line_indent_columns(text, current_line, tabstop);

    let count = ctx.count_usize();
    let mut found = 0;

    let mut line = current_line;
    loop {
        match direction {
            ScanDir::Up => {
                if line == 0 {
                    break;
                }
                line -= 1;
            }
            ScanDir::Down => {
                line += 1;
                if line >= total_lines {
                    break;
                }
            }
        }

        // Skip blank lines — they don't have meaningful indentation
        if is_blank_line(text, line_start(text, line).unwrap_or(0)) {
            continue;
        }

        let candidate_indent = line_indent_columns(text, line, tabstop);
        let matches = match comparison {
            IndentCmp::Same => candidate_indent == reference_indent,
            IndentCmp::Lesser => candidate_indent < reference_indent,
            IndentCmp::Greater => candidate_indent > reference_indent,
        };

        if matches {
            found += 1;
            if found >= count {
                let ls = line_start(text, line).unwrap_or(0);
                let content = line_content(text, line).unwrap_or("");
                let target_offset = ls + first_non_blank_in_line(content);
                return MotionResult::Position(Offset::new(target_offset));
            }
        }
    }

    MotionResult::Error
}

/// Measure a line's indentation in display columns.
///
/// Tabs expand to the next `tabstop` boundary. Spaces count as 1 column.
/// Stops at first non-whitespace character or end of line.
fn line_indent_columns(text: &str, line_num: usize, tabstop: usize) -> usize {
    let start = line_start(text, line_num).unwrap_or(0);
    let end = line_end(text, line_num).unwrap_or(text.len());
    let line_text = &text[start..end];

    let tabstop = tabstop.max(1); // Prevent division by zero
    let mut col = 0;
    for byte in line_text.bytes() {
        match byte {
            b' ' => col += 1,
            b'\t' => col = (col / tabstop + 1) * tabstop,
            _ => break,
        }
    }
    col
}
