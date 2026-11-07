//! Global command (`:g/pattern/cmd`, `:v/pattern/cmd`): the lines it marks
//! and how those marks follow the text.
//!
//! The engine runs the commands, one marked line at a time, on the text the
//! line before left (ex_cmds.c `ex_global()`, `global_exe()`).

use super::range::resolve_range;
use super::types::ExContext;
use crate::errors::VimError;
use crate::grammar::types::ExRange;
use crate::regex::{Cache, MatchContext, VimRegex};

/// Maximum number of matching lines `:g` will process.
pub(crate) const MAX_GLOBAL_CMD_LINES: usize = 100_000;

/// The lines of `range` that match `pattern`, or with `invert` that do not,
/// 0-indexed: pass 1 of `ex_global()`.
///
/// # Errors
///
/// A bad range or pattern, or more than [`MAX_GLOBAL_CMD_LINES`] lines.
pub(crate) fn global_lines(
    range: &ExRange,
    pattern: &str,
    invert: bool,
    ctx: &ExContext,
) -> Result<Vec<usize>, VimError> {
    let resolved = resolve_range(range, ctx)?;
    let re =
        VimRegex::new(pattern).map_err(|e| VimError::PatternNotFound(format!("{e}").into()))?;
    let mut cache = re.create_cache_no_captures();
    // A line without the pattern's literal cannot match.
    let candidates: Option<Vec<usize>> = ctx.tree.and_then(|tree| {
        re.bloom_literal()
            .map(|literal| tree.find_matching_lines(literal, resolved.start()..resolved.end() + 1))
    });
    let mut next_candidate = 0usize;
    let mut lines = Vec::new();
    for line in resolved.start()..=resolved.end() {
        if let Some(candidates) = &candidates {
            while candidates.get(next_candidate).is_some_and(|&c| c < line) {
                next_candidate += 1;
            }
            if candidates.get(next_candidate) != Some(&line) {
                if invert {
                    lines.push(line);
                }
                continue;
            }
        }
        if line_matches(&re, &mut cache, ctx.line_text(line)) != invert {
            lines.push(line);
        }
    }
    if lines.len() > MAX_GLOBAL_CMD_LINES {
        return Err(VimError::GlobalLineLimitExceeded {
            limit: MAX_GLOBAL_CMD_LINES,
        });
    }
    Ok(lines)
}

/// Whether the line `line` of `ctx` matches `pattern`, as a `:g` inside a
/// `:g` checks the cursor line (`ex_global()` with `global_busy`).
///
/// # Errors
///
/// A bad pattern.
pub(crate) fn line_matches_pattern(
    pattern: &str,
    line: usize,
    ctx: &ExContext,
) -> Result<bool, VimError> {
    let re =
        VimRegex::new(pattern).map_err(|e| VimError::PatternNotFound(format!("{e}").into()))?;
    let mut cache = re.create_cache_no_captures();
    Ok(line_matches(&re, &mut cache, ctx.line_text(line)))
}

fn line_matches(re: &VimRegex, cache: &mut Cache, text: Option<&str>) -> bool {
    text.is_some_and(|text| {
        re.is_match_with_cache(cache, &MatchContext::simple(text))
            .unwrap_or(false)
    })
}

/// The lines `:g` has yet to run on, as Vim's memline marks them
/// (`ml_setmarked()`): a line keeps its mark while its text changes and
/// loses it when it is deleted, and a line added has none.
///
/// Lines are the text's `\n`-separated pieces, the empty one after a last
/// `\n` included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineMarks {
    marked: Vec<bool>,
    /// No line before this one is marked.
    low: usize,
}

impl LineMarks {
    /// Mark `lines` of a text of `line_count` lines.
    pub(crate) fn new(line_count: usize, lines: &[usize]) -> Self {
        let mut marked = vec![false; line_count];
        for &line in lines {
            if let Some(mark) = marked.get_mut(line) {
                *mark = true;
            }
        }
        Self { marked, low: 0 }
    }

    /// Take the mark of the first marked line and return that line
    /// (`ml_firstmarked()`).
    pub(crate) fn take_first(&mut self) -> Option<usize> {
        let found = self.marked.get(self.low..)?.iter().position(|&m| m)?;
        let line = self.low + found;
        if let Some(mark) = self.marked.get_mut(line) {
            *mark = false;
        }
        self.low = line + 1;
        Some(line)
    }

    /// Follow the edit of `text` that replaces `start..end` with `new`.
    ///
    /// An edit from the start of a line to the start of a line that adds
    /// nothing or whole lines deletes and adds whole lines, so the line
    /// after it keeps its mark, as after `ml_delete()` and `ml_append()`.
    /// Any other edit changes the line it starts in, which keeps its mark
    /// (`ml_replace()`), and takes the mark of the other lines it reaches
    /// into.
    pub(crate) fn edit(&mut self, text: &str, start: usize, end: usize, new: &str) {
        let start = start.min(text.len());
        let end = end.clamp(start, text.len());
        let first = newlines(text.get(..start).unwrap_or_default());
        let last = first + newlines(text.get(start..end).unwrap_or_default());
        let added = newlines(new);
        let at_line_start =
            |offset: usize| offset == 0 || text.as_bytes().get(offset - 1) == Some(&b'\n');
        let whole_lines = new.is_empty() || new.ends_with('\n');
        let (from, to) = if whole_lines && at_line_start(start) && at_line_start(end) {
            (first, last)
        } else {
            (first + 1, last + 1)
        };
        let to = to.min(self.marked.len());
        let from = from.min(to);
        self.marked
            .splice(from..to, std::iter::repeat_n(false, added));
        self.low = self.low.min(first);
    }
}

fn newlines(text: &str) -> usize {
    memchr::memchr_iter(b'\n', text.as_bytes()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_of(text: &str, range: &ExRange, pattern: &str, invert: bool) -> Vec<usize> {
        global_lines(range, pattern, invert, &ExContext::new(text, 0)).unwrap()
    }

    #[test]
    fn marks_the_matching_lines_of_the_range() {
        let text = "line one\nTODO fix\nline three\nTODO another";
        let all = ExRange::entire_file();
        assert_eq!(lines_of(text, &all, "TODO", false), vec![1, 3]);
        assert_eq!(lines_of(text, &all, "TODO", true), vec![0, 2]);
        let two = ExRange::lines(2, 3);
        assert_eq!(lines_of(text, &two, "TODO", false), vec![1]);
    }

    /// Vim's buffer holds an empty last line where the text ends in `\n`,
    /// and `^` matches it.
    #[test]
    fn the_empty_line_after_a_last_newline_is_a_line() {
        let all = ExRange::entire_file();
        assert_eq!(lines_of("a\nb\n", &all, "^", false), vec![0, 1, 2]);
        assert_eq!(lines_of("a\nb\n", &all, ".", false), vec![0, 1]);
    }

    #[test]
    fn a_bad_pattern_is_an_error() {
        let ctx = ExContext::new("a\n", 0);
        assert!(global_lines(&ExRange::entire_file(), "\\(", false, &ctx).is_err());
    }

    fn marked(marks: &LineMarks) -> Vec<usize> {
        let mut marks = marks.clone();
        std::iter::from_fn(|| marks.take_first()).collect()
    }

    #[test]
    fn a_deleted_line_loses_its_mark() {
        let text = "a\nb\nc\nd";
        let mut marks = LineMarks::new(4, &[0, 1, 2, 3]);
        // `:2d`: delete "b\n".
        marks.edit(text, 2, 4, "");
        assert_eq!(marked(&marks), vec![0, 1, 2]);
        // `:$d` of "a\nc\nd": delete "\nd".
        let mut marks = LineMarks::new(3, &[0, 1, 2]);
        marks.edit("a\nc\nd", 3, 5, "");
        assert_eq!(marked(&marks), vec![0, 1]);
    }

    #[test]
    fn an_added_line_has_no_mark() {
        let mut marks = LineMarks::new(3, &[0, 1, 2]);
        // `:2m0` of "a\nb\nc": insert "b\n" at 0, then delete the old "b\n".
        marks.edit("a\nb\nc", 0, 0, "b\n");
        assert_eq!(marked(&marks), vec![1, 2, 3]);
        marks.edit("b\na\nb\nc", 4, 6, "");
        assert_eq!(marked(&marks), vec![1, 2]);
        // A line added after the last one, which has no `\n`.
        let mut marks = LineMarks::new(2, &[0, 1]);
        marks.edit("a\nb", 3, 3, "\nx");
        assert_eq!(marked(&marks), vec![0, 1]);
        // A line added after an empty last line.
        let mut marks = LineMarks::new(3, &[0, 1, 2]);
        marks.edit("a\nb\n", 4, 4, "\nx");
        assert_eq!(marked(&marks), vec![0, 1, 2]);
    }

    #[test]
    fn a_changed_line_keeps_its_mark() {
        let mut marks = LineMarks::new(3, &[0, 2]);
        // `:s/b/xyz/` on line 2.
        marks.edit("a\nb\nc", 2, 3, "xyz");
        assert_eq!(marked(&marks), vec![0, 2]);
        // `:j` of lines 1 and 2: the first keeps its mark, the second goes.
        let mut marks = LineMarks::new(3, &[0, 1, 2]);
        marks.edit("a\nb\nc", 1, 2, " ");
        assert_eq!(marked(&marks), vec![0, 1]);
        // A line split in two: the new second half has none.
        let mut marks = LineMarks::new(2, &[0, 1]);
        marks.edit("ab\nc", 1, 1, "\n");
        assert_eq!(marked(&marks), vec![0, 2]);
    }

    #[test]
    fn take_first_takes_lines_in_order() {
        let mut marks = LineMarks::new(5, &[1, 3]);
        assert_eq!(marks.take_first(), Some(1));
        assert_eq!(marks.take_first(), Some(3));
        assert_eq!(marks.take_first(), None);
    }

    #[test]
    fn an_edit_before_the_next_mark_moves_it() {
        let mut marks = LineMarks::new(4, &[0, 2, 3]);
        assert_eq!(marks.take_first(), Some(0));
        // Two lines added before line 1.
        marks.edit("a\nb\nc\nd", 2, 2, "x\ny\n");
        assert_eq!(marks.take_first(), Some(4));
        assert_eq!(marks.take_first(), Some(5));
    }
}
