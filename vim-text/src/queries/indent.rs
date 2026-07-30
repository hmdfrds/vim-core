//! Indentation-query helper functions for VimText.
//!
//! Extracted from `blank_lines.rs`. Called by the `VimQueries` impl.
//!
//! Uses IndentSummary tree aggregation for interior chunks, scanning
//! only boundary chunks. Total complexity: O(log n + 2*CHUNK_MAX).

use crate::summary::{ByteOffset, IndentFlags};
use crate::tree::Bias;
use crate::VimText;

/// Scan the byte range `[byte_start, byte_end)` and return the minimum
/// indentation among non-blank lines.
///
/// "Indent" = count of leading spaces/tabs (each tab counts as 1 unit).
/// Blank lines (only whitespace or empty) are ignored.
/// Returns `u16::MAX` if all lines in the range are blank.
///
/// **Precondition:** `byte_start` is at a line boundary (the start of a line).
/// This is always true when called from `min_indent_in_range`, which converts
/// line numbers to byte offsets via `line_start()`.
///
/// **Complexity:** O(log n + 2*CHUNK_MAX). For interior chunks that are fully
/// contained in the range AND start at a line boundary, we read min_indent
/// from the chunk's IndentSummary in O(1). Only the first and last boundary
/// chunks (and any interior chunk whose first line is a continuation) are
/// scanned byte-by-byte.
///
/// Uses `Bias::Right` for the initial cursor seek so that
/// seeking to a chunk boundary lands on the chunk that *starts* there.
pub(crate) fn min_indent_in_byte_range(text: &VimText, byte_start: usize, byte_end: usize) -> u16 {
    if byte_start >= byte_end {
        return u16::MAX;
    }

    let mut cursor = text.tree.cursor::<ByteOffset>();
    cursor.seek(&ByteOffset(byte_start as u32), Bias::Right);

    let mut result = u16::MAX;
    // The byte range starts at a line boundary (precondition), so the
    // first chunk always begins at a line start within our range.
    let mut prev_ended_with_newline = true;
    let mut is_first = true;

    while let Some(chunk) = cursor.item() {
        let summary = cursor.item_summary().unwrap();
        let chunk_start = cursor.start::<ByteOffset>().0 as usize;
        let chunk_end = chunk_start + summary.metrics.bytes as usize;

        if chunk_start >= byte_end {
            break;
        }

        let is_fully_contained = chunk_start >= byte_start && chunk_end <= byte_end;

        if is_fully_contained && !is_first && prev_ended_with_newline {
            // Interior chunk that starts at a line boundary — use summary
            // in O(1). The summary's min_indent covers all non-blank lines
            // in the chunk, and since the first line starts at a boundary,
            // there is no continuation-line pollution.
            if summary.has_nonblank_line() {
                // Chunk has at least one non-blank line, so min_indent is
                // meaningful (not u16::MAX).
                result = result.min(summary.indent.min_indent);
            }
            // If all lines are blank, min_indent is u16::MAX — skip.

            prev_ended_with_newline = summary
                .indent
                .flags
                .contains(IndentFlags::ENDS_WITH_NEWLINE);
        } else {
            // Boundary chunk (first/last/partial) or interior chunk whose
            // first line is a continuation — fall back to byte scanning.
            let local_start = byte_start.saturating_sub(chunk_start);
            let local_end = (byte_end - chunk_start).min(chunk.byte_len());
            let bytes = &chunk.as_bytes()[local_start..local_end];

            // Determine if we're starting mid-line (continuation from
            // previous chunk). If so, skip the continuation segment.
            let at_line_start = prev_ended_with_newline || is_first;
            result = result.min(scan_indent_in_bytes(bytes, at_line_start));

            // Determine if this chunk slice ends with \n.
            prev_ended_with_newline = bytes.last() == Some(&b'\n');
        }

        if result == 0 {
            return 0; // Can't get lower than 0.
        }

        is_first = false;

        if !cursor.next() {
            break;
        }
    }

    result
}

/// Scan bytes for minimum indent of non-blank lines.
///
/// `at_line_start`: if true, the first byte is at the start of a line
/// (indent counting begins immediately). If false, the first bytes are a
/// continuation of a line from a previous chunk — skip to the first `\n`.
fn scan_indent_in_bytes(bytes: &[u8], at_line_start: bool) -> u16 {
    let mut min_indent = u16::MAX;
    let mut counting_indent = at_line_start;
    let mut current_indent: u16 = 0;
    let mut line_has_content = false;
    // When starting mid-line (continuation from previous chunk), skip all
    // bytes until the first \n — the continuation segment is not a real line
    // start and must not be counted.
    let mut skip_to_newline = !at_line_start;

    for &b in bytes {
        if b == b'\n' {
            if !skip_to_newline && line_has_content {
                min_indent = min_indent.min(current_indent);
                if min_indent == 0 {
                    return 0;
                }
            }
            skip_to_newline = false;
            counting_indent = true;
            current_indent = 0;
            line_has_content = false;
        } else if skip_to_newline {
            // Still in the continuation segment — ignore everything.
        } else if counting_indent {
            if b == b' ' || b == b'\t' {
                current_indent += 1;
            } else if b != b'\r' {
                counting_indent = false;
                line_has_content = true;
            }
        } else if !line_has_content && b != b' ' && b != b'\t' && b != b'\r' {
            line_has_content = true;
        }
    }

    // Handle last line in the slice (no trailing \n).
    if !skip_to_newline && line_has_content {
        min_indent = min_indent.min(current_indent);
    }

    min_indent
}
