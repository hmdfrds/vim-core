//! Ex command range resolution.
//!
//! Resolves `ExRange` and `LineSpec` to concrete line numbers.

use super::types::ExContext;
use crate::commands::helpers;
use crate::errors::VimError;
use crate::grammar::types::{ExRange, LineSpec, RangeSeparator};
use crate::primitives::byte_delta;
use crate::primitives::Direction;
use crate::regex::{MatchContext, VimRegex};

use super::types::ResolvedRange;

/// Validate a signed line number against the document and convert it.
///
/// A negative line number, or one at or past `total_lines`, is out of range.
/// `try_from` rejects the negative case, so the conversion never loses a sign.
fn line_in_document(line: i64, total_lines: usize) -> Result<usize, VimError> {
    match usize::try_from(line) {
        Ok(l) if l < total_lines => Ok(l),
        _ => Err(VimError::InvalidRange),
    }
}

/// Resolve a `LineSpec` to a concrete line number (0-indexed).
///
/// # Errors
///
/// Returns `VimError::InvalidAddress` or `VimError::InvalidRange` for invalid specs.
pub fn resolve_line_spec(spec: &LineSpec, ctx: &ExContext) -> Result<usize, VimError> {
    match spec {
        LineSpec::Current => Ok(ctx.cursor_line),
        LineSpec::Last => Ok(ctx.total_lines.saturating_sub(1)),
        LineSpec::Absolute(n) => {
            let line = (*n as usize).saturating_sub(1); // 1-indexed to 0-indexed
            if line >= ctx.total_lines {
                Err(VimError::InvalidAddress(format!("{n}").into()))
            } else {
                Ok(line)
            }
        }
        LineSpec::Relative(offset) => {
            let line = byte_delta::to_i64(ctx.cursor_line).saturating_add(i64::from(*offset));
            line_in_document(line, ctx.total_lines)
        }
        LineSpec::Mark(name) => {
            if let Some(resolve_mark) = ctx.mark_resolver {
                if let Some(offset) = resolve_mark(*name) {
                    let clamped = offset.min(ctx.text.len());
                    Ok(helpers::line_of(ctx.text, clamped))
                } else {
                    Err(VimError::MarkNotSet(name.char()))
                }
            } else {
                Err(VimError::InvalidAddress(format!("'{name}'").into()))
            }
        }
        LineSpec::SearchForward(pattern) => resolve_search_line(ctx, pattern, Direction::Forward),
        LineSpec::SearchBackward(pattern) => resolve_search_line(ctx, pattern, Direction::Backward),
        LineSpec::WithOffset { base, offset } => {
            let base_line = resolve_line_spec(base, ctx)?;
            let line = byte_delta::to_i64(base_line).saturating_add(i64::from(*offset));
            line_in_document(line, ctx.total_lines)
        }
    }
}

fn resolve_search_line(
    ctx: &ExContext,
    pattern: &str,
    direction: Direction,
) -> Result<usize, VimError> {
    let re = VimRegex::new(pattern).map_err(|_| VimError::PatternNotFound(pattern.into()))?;
    let mut cache = re.create_cache_no_captures();
    let total = ctx.total_lines.max(1);

    if direction.is_forward() {
        for line in ((ctx.cursor_line + 1)..total).chain(0..=ctx.cursor_line.min(total - 1)) {
            if let Some(line_text) = ctx.line_text(line) {
                let line_ctx = MatchContext::simple(line_text);
                if re
                    .is_match_with_cache(&mut cache, &line_ctx)
                    .unwrap_or(false)
                {
                    return Ok(line);
                }
            }
        }
    } else {
        let mut line = if ctx.cursor_line == 0 {
            total - 1
        } else {
            ctx.cursor_line - 1
        };
        for _ in 0..total {
            if let Some(line_text) = ctx.line_text(line) {
                let line_ctx = MatchContext::simple(line_text);
                if re
                    .is_match_with_cache(&mut cache, &line_ctx)
                    .unwrap_or(false)
                {
                    return Ok(line);
                }
            }
            line = if line == 0 { total - 1 } else { line - 1 };
        }
    }

    Err(VimError::PatternNotFound(pattern.into()))
}

/// Resolve an `ExRange` to a concrete line range.
///
/// When the separator is `;`, the cursor is logically moved to `start` before
/// the second address is evaluated. This implements Vim's behaviour where
/// `5;/foo/` searches for `/foo/` starting from line 5 rather than from the
/// actual cursor position.
///
/// # Errors
///
/// Returns `VimError::InvalidAddress` or `VimError::InvalidRange` for invalid ranges.
pub fn resolve_range(range: &ExRange, ctx: &ExContext) -> Result<ResolvedRange, VimError> {
    let start = resolve_line_spec(&range.start, ctx)?;
    let end = match &range.end {
        None => start, // Single line
        Some(end_spec) => {
            if range.separator == RangeSeparator::Semicolon {
                // With `;`, evaluate the second address with cursor set to start.
                let mut ctx_at_start = *ctx;
                ctx_at_start.cursor_line = start;
                resolve_line_spec(end_spec, &ctx_at_start)?
            } else {
                resolve_line_spec(end_spec, ctx)?
            }
        }
    };

    // Silently swap backwards ranges (matches Neovim behavior).
    let (start, end) = if start > end {
        (end, start)
    } else {
        (start, end)
    };

    if end >= ctx.total_lines {
        return Err(VimError::InvalidRange);
    }

    Ok(ResolvedRange::new(start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ExContext<'static> {
        ExContext::new("line1\nline2\nline3\nline4\nline5", 2) // cursor on line3
    }

    #[test]
    fn test_resolve_current() {
        let line = resolve_line_spec(&LineSpec::Current, &ctx()).unwrap();
        assert_eq!(line, 2);
    }

    #[test]
    fn test_resolve_last() {
        let line = resolve_line_spec(&LineSpec::Last, &ctx()).unwrap();
        assert_eq!(line, 4);
    }

    #[test]
    fn test_resolve_absolute() {
        let line = resolve_line_spec(&LineSpec::Absolute(3), &ctx()).unwrap();
        assert_eq!(line, 2); // 1-indexed to 0-indexed
    }

    #[test]
    fn test_resolve_relative() {
        let line = resolve_line_spec(&LineSpec::Relative(1), &ctx()).unwrap();
        assert_eq!(line, 3);

        let line = resolve_line_spec(&LineSpec::Relative(-1), &ctx()).unwrap();
        assert_eq!(line, 1);
    }

    #[test]
    fn test_resolve_range() {
        let range = ExRange::lines(2, 4);
        let resolved = resolve_range(&range, &ctx()).unwrap();
        assert_eq!(resolved.start(), 1);
        assert_eq!(resolved.end(), 3);
        assert_eq!(resolved.line_count(), 3);
    }

    #[test]
    fn test_entire_file_range() {
        let range = ExRange::entire_file();
        let resolved = resolve_range(&range, &ctx()).unwrap();
        assert_eq!(resolved.start(), 0);
        assert_eq!(resolved.end(), 4);
    }

    #[test]
    fn test_backwards_range_swapped() {
        // :5,2 → swapped to 2..5 (0-indexed: 1..4)
        let range = ExRange::lines(5, 2);
        let resolved = resolve_range(&range, &ctx()).unwrap();
        assert_eq!(resolved.start(), 1);
        assert_eq!(resolved.end(), 4);
    }

    #[test]
    fn test_backwards_range_adjacent() {
        // :3,1 → swapped to 1..3 (0-indexed: 0..2)
        let range = ExRange::lines(3, 1);
        let resolved = resolve_range(&range, &ctx()).unwrap();
        assert_eq!(resolved.start(), 0);
        assert_eq!(resolved.end(), 2);
        assert_eq!(resolved.line_count(), 3);
    }

    #[test]
    fn test_forward_range_unchanged() {
        // :2,4 → normal forward range (0-indexed: 1..3)
        let range = ExRange::lines(2, 4);
        let resolved = resolve_range(&range, &ctx()).unwrap();
        assert_eq!(resolved.start(), 1);
        assert_eq!(resolved.end(), 3);
    }
}
