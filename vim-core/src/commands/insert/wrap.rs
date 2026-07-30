use crate::commands::helpers::line_start_for_offset;

/// The effects needed to wrap a line at the textwidth boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapAction {
    /// Byte offset of the whitespace character to replace with a newline.
    pub break_offset: usize,
    /// Number of whitespace bytes at the break point to delete.
    pub break_len: usize,
    /// The indent string to prepend to the wrapped continuation line.
    pub continuation_indent: String,
}

/// Compute auto-format wrap action for a character about to be inserted.
///
/// Called BEFORE the character is applied to the document. Simulates the
/// post-edit line (`line_prefix + ch + line_suffix`) and checks if it
/// exceeds `textwidth`. If so, finds the rightmost space at or before
/// the textwidth column and returns a `WrapAction`.
///
/// `text` is the T0 (pre-edit) document. `cursor` is where the char
/// will be inserted. The function builds the simulated post-edit line
/// to check against textwidth.
///
/// Returns `None` if no wrap is needed (line fits, no break point found,
/// or the inserted character is whitespace).
#[must_use]
pub fn compute_auto_format_wrap(
    text: &str,
    cursor: usize,
    ch: char,
    textwidth: usize,
) -> Option<WrapAction> {
    if textwidth == 0 || ch.is_whitespace() {
        return None;
    }

    let line_start = line_start_for_offset(text, cursor);
    let line_end = text[cursor..].find('\n').map_or(text.len(), |n| cursor + n);

    // Simulate post-edit line: prefix + new char + suffix
    let prefix = &text[line_start..cursor];
    let suffix = &text[cursor..line_end];
    let mut simulated = String::with_capacity(prefix.len() + ch.len_utf8() + suffix.len());
    simulated.push_str(prefix);
    simulated.push(ch);
    simulated.push_str(suffix);

    let display_cols = simulated.chars().count();

    if display_cols <= textwidth {
        return None;
    }

    // Find the rightmost whitespace at or before the textwidth column
    // in the simulated line. All offsets are relative to `line_start` in
    // the original document.
    let mut last_ws_sim_offset = None;
    let mut col = 0;
    let mut sim_byte = 0;

    for c in simulated.chars() {
        if col > textwidth {
            break;
        }
        if c.is_whitespace() && col > 0 {
            last_ws_sim_offset = Some(sim_byte);
        }
        sim_byte += c.len_utf8();
        col += 1;
    }

    let sim_break = last_ws_sim_offset?;

    // Map simulated offset back to document offset.
    // The inserted char sits at `cursor - line_start` in the simulated string.
    // Before that point: sim offset == doc offset relative to line_start.
    // At or after: doc offset = sim offset - ch.len_utf8() (the char isn't in T0).
    let insert_pos_in_sim = cursor - line_start;
    let doc_break = if sim_break < insert_pos_in_sim {
        line_start + sim_break
    } else {
        line_start + sim_break - ch.len_utf8()
    };

    // Count whitespace bytes at break point in the original document
    let mut break_len = 0;
    for b in text[doc_break..].bytes() {
        if b == b' ' || b == b'\t' {
            break_len += 1;
        } else {
            break;
        }
    }
    if break_len == 0 {
        break_len = 1;
    }

    // Continuation indent: copy leading whitespace from the current line
    let line = &text[line_start..line_end];
    let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let continuation_indent = text[line_start..line_start + indent_len].to_string();

    Some(WrapAction {
        break_offset: doc_break,
        break_len,
        continuation_indent,
    })
}

/// Find the break point for auto-wrapping a line that exceeds textwidth.
///
/// Returns `Some(byte_offset)` of the last whitespace at or before
/// the textwidth column. Returns `None` if no wrap needed or no
/// whitespace found (long unbreakable word).
#[must_use]
pub fn find_wrap_point(line_text: &str, textwidth: usize) -> Option<usize> {
    if textwidth == 0 || line_text.len() <= textwidth {
        return None;
    }
    let search_region = &line_text[..textwidth.min(line_text.len())];
    search_region.rfind(|c: char| c.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_wrap_when_under_limit() {
        assert_eq!(find_wrap_point("short", 80), None);
    }

    #[test]
    fn no_wrap_when_exactly_at_limit() {
        assert_eq!(find_wrap_point("12345", 5), None);
    }

    #[test]
    fn wraps_at_last_space_before_limit() {
        // "hello world this is a long line" with textwidth=15
        // Last space before col 15 is at index 11 (before "this")
        let text = "hello world this is a long line";
        assert_eq!(find_wrap_point(text, 15), Some(11));
    }

    #[test]
    fn no_wrap_for_long_word() {
        assert_eq!(find_wrap_point("averylongwordwithoutspaces", 10), None);
    }

    #[test]
    fn no_wrap_when_textwidth_zero() {
        assert_eq!(find_wrap_point("hello world", 0), None);
    }

    #[test]
    fn wraps_at_closest_space_to_limit() {
        // "aa bb cc dd ee" with textwidth=10
        // Last space at or before col 10 is at index 8 (before "dd")
        let text = "aa bb cc dd ee";
        assert_eq!(find_wrap_point(text, 10), Some(8));
    }

    #[test]
    fn single_space_at_start() {
        assert_eq!(find_wrap_point(" longword", 5), Some(0));
    }
}
