//! Motions on which Vim cancels the format operators.
//!
//! Vim cancels an operator when its motion fails, as `2)` does in the last
//! sentence of the buffer or `ge` at its first character. The engine's
//! motions stop at the edge of the buffer instead. The format operators
//! format every line the range touches, so on such a motion they would
//! format lines that Vim leaves alone. They ask here first.
//!
//! The sentence and paragraph checks follow Vim 9.1's `findsent()` and
//! `findpar()` (textobject.c) far enough to tell whether they fail.

use super::format::starts_paragraph_or_section;
use crate::grammar::types::Motion;

/// What Vim does with the cursor when a motion fails for an operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cancel {
    /// The cursor stays where it was.
    Stay,
    /// The cursor stays where the motion got before it failed: the start of
    /// the buffer for the backward word motions, which move it as they go.
    BufferStart,
}

/// Whether Vim's motion fails for an operator, so that `gq` or `gw` does
/// nothing, and what that leaves the cursor at. `target(n)` is where the
/// engine's motion goes with count `n`, `None` when it fails too.
pub(super) fn motion_fails(
    text: &str,
    cursor: usize,
    motion: Motion,
    count: u32,
    target: impl Fn(u32) -> Option<usize>,
) -> Option<Cancel> {
    let backward_word = matches!(
        motion,
        Motion::WordBackward
            | Motion::WORDBackward
            | Motion::WordEndBackward
            | Motion::WORDEndBackward
    );
    fails(text, cursor, motion, count, target).then_some(if backward_word {
        Cancel::BufferStart
    } else {
        Cancel::Stay
    })
}

fn fails(
    text: &str,
    cursor: usize,
    motion: Motion,
    count: u32,
    target: impl Fn(u32) -> Option<usize>,
) -> bool {
    let count = count.max(1);
    let steps = count as usize;
    let buffer = Buffer::new(text);
    let (line, _) = buffer.pos(cursor);
    let last_line = buffer.lines.len() - 1;
    match motion {
        // Vim's cursor_up() and cursor_down() fail only when the cursor
        // cannot move at all.
        Motion::UpFirstNonBlank => line == 0,
        Motion::DownFirstNonBlank => line == last_line,
        // `_` and `$` move count - 1 lines down.
        Motion::FirstNonBlankLine | Motion::LineEnd => count > 1 && line == last_line,
        // bck_word() and bckend_word() fail when a step starts at the
        // start of the buffer. A step that reaches it on the way stops
        // there and succeeds, so only the first step can start there, or a
        // later one when an empty first line stopped the step before.
        Motion::WordBackward
        | Motion::WORDBackward
        | Motion::WordEndBackward
        | Motion::WORDEndBackward => {
            cursor == 0
                || (count > 1 && buffer.line_empty(0) && target(count - 1).is_none_or(|t| t == 0))
        }
        Motion::SentenceForward => !buffer.find_sentence(line_col(&buffer, cursor), true, steps),
        Motion::SentenceBackward => !buffer.find_sentence(line_col(&buffer, cursor), false, steps),
        Motion::ParagraphForward => !buffer.find_paragraph(line, true, steps),
        Motion::ParagraphBackward => !buffer.find_paragraph(line, false, steps),
        _ => false,
    }
}

fn line_col(buffer: &Buffer<'_>, offset: usize) -> Pos {
    let (line, col) = buffer.pos(offset);
    Pos { line, col }
}

/// A position as Vim keeps it: a line and a byte column, where the column
/// may be the end of the line (Vim's NUL).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pos {
    line: usize,
    col: usize,
}

struct Buffer<'t> {
    lines: Vec<&'t str>,
}

impl<'t> Buffer<'t> {
    fn new(text: &'t str) -> Self {
        Self {
            lines: text.split('\n').collect(),
        }
    }

    /// Line and byte column of `offset`.
    fn pos(&self, offset: usize) -> (usize, usize) {
        let mut start = 0;
        for (i, line) in self.lines.iter().enumerate() {
            let end = start + line.len();
            if offset <= end || i == self.lines.len() - 1 {
                return (i, offset.saturating_sub(start).min(line.len()));
            }
            start = end + 1;
        }
        (0, 0)
    }

    fn line(&self, line: usize) -> &'t str {
        self.lines.get(line).copied().unwrap_or("")
    }

    /// The character at `p`, or `None` at the end of its line (Vim's NUL).
    fn char_at(&self, p: Pos) -> Option<char> {
        self.line(p.line)
            .get(p.col..)
            .and_then(|s| s.chars().next())
    }

    fn line_empty(&self, line: usize) -> bool {
        self.line(line).is_empty()
    }

    /// Vim's inc(): -1 at the end of the buffer, 1 when it moved to the next
    /// line, 2 when it moved onto the end of the line, else 0.
    fn inc(&self, p: &mut Pos) -> i32 {
        if let Some(c) = self.char_at(*p) {
            p.col += c.len_utf8();
            return if self.char_at(*p).is_some() { 0 } else { 2 };
        }
        if p.line + 1 < self.lines.len() {
            p.line += 1;
            p.col = 0;
            return 1;
        }
        -1
    }

    /// Vim's incl(): inc() that skips the end of a non-empty line.
    fn incl(&self, p: &mut Pos) -> i32 {
        let r = self.inc(p);
        if r >= 1 && p.col > 0 {
            self.inc(p)
        } else {
            r
        }
    }

    /// Vim's dec(): -1 at the start of the buffer, 1 when it moved to the end
    /// of the line above, else 0.
    fn dec(&self, p: &mut Pos) -> i32 {
        if p.col > 0 {
            let line = self.line(p.line);
            let col = p.col.min(line.len());
            p.col = line
                .get(..col)
                .and_then(|s| s.char_indices().next_back())
                .map_or(0, |(i, _)| i);
            return 0;
        }
        if p.line > 0 {
            p.line -= 1;
            p.col = self.line(p.line).len();
            return 1;
        }
        -1
    }

    /// Vim's decl(): dec() that skips the end of a non-empty line.
    fn decl(&self, p: &mut Pos) -> i32 {
        let r = self.dec(p);
        if r == 1 && p.col > 0 {
            self.dec(p)
        } else {
            r
        }
    }

    fn step(&self, p: &mut Pos, forward: bool) -> i32 {
        if forward {
            self.incl(p)
        } else {
            self.decl(p)
        }
    }

    fn starts_ps(&self, line: usize) -> bool {
        starts_paragraph_or_section(self.line(line))
    }

    /// Vim's findsent() from `pos`: whether it succeeds.
    fn find_sentence(&self, mut pos: Pos, forward: bool, count: usize) -> bool {
        let mut count = count;
        while count > 0 {
            count -= 1;
            let prev = pos;
            let mut noskip = false;
            'found: {
                if self.char_at(pos).is_none() {
                    // On an empty line (or the end of one): skip to a
                    // non-empty line.
                    loop {
                        if self.step(&mut pos, forward) == -1 {
                            break;
                        }
                        if self.char_at(pos).is_some() {
                            break;
                        }
                    }
                    if forward {
                        break 'found;
                    }
                } else if forward && pos.col == 0 && self.starts_ps(pos.line) {
                    if pos.line + 1 == self.lines.len() {
                        return false;
                    }
                    pos.line += 1;
                    pos.col = 0;
                    break 'found;
                } else if !forward {
                    self.decl(&mut pos);
                }

                // Go back to the previous non-white non-punctuation character.
                let mut found_dot = false;
                while let Some(c) = self
                    .char_at(pos)
                    .filter(|&c| c == ' ' || c == '\t' || ".!?)]\"'".contains(c))
                {
                    let mut t = pos;
                    if self.decl(&mut t) == -1 || (self.line_empty(t.line) && forward) {
                        break;
                    }
                    if found_dot {
                        break;
                    }
                    if ".!?".contains(c) {
                        found_dot = true;
                    }
                    if ")]\"'".contains(c)
                        && !self.char_at(t).is_some_and(|tc| ".!?)]\"'".contains(tc))
                    {
                        break;
                    }
                    self.decl(&mut pos);
                }

                // Find the end of the sentence.
                let start_line = pos.line;
                loop {
                    let c = self.char_at(pos);
                    if c.is_none() || (pos.col == 0 && self.starts_ps(pos.line)) {
                        if !forward && pos.line != start_line {
                            pos.line += 1;
                            pos.col = 0;
                        }
                        break;
                    }
                    if matches!(c, Some('.' | '!' | '?')) {
                        let mut t = pos;
                        let mut r;
                        loop {
                            r = self.inc(&mut t);
                            if r == -1 {
                                break;
                            }
                            if !self.char_at(t).is_some_and(|tc| ")]\"'".contains(tc)) {
                                break;
                            }
                        }
                        let tc = self.char_at(t);
                        if r == -1 || matches!(tc, None | Some(' ' | '\t')) {
                            pos = t;
                            if self.char_at(pos).is_none() {
                                self.inc(&mut pos);
                            }
                            break;
                        }
                    }
                    if self.step(&mut pos, forward) == -1 {
                        if count > 0 {
                            return false;
                        }
                        noskip = true;
                        break;
                    }
                }
            }
            // Skip white space.
            while !noskip && matches!(self.char_at(pos), Some(' ' | '\t')) {
                if self.incl(&mut pos) == -1 {
                    break;
                }
            }
            if pos == prev {
                // Did not move: try again.
                if self.step(&mut pos, forward) == -1 {
                    if count > 0 {
                        return false;
                    }
                    break;
                }
                count += 1;
            }
        }
        true
    }

    /// Vim's findpar() from `line`: whether it succeeds.
    fn find_paragraph(&self, line: usize, forward: bool, count: usize) -> bool {
        let mut curr = line;
        let last = self.lines.len() - 1;
        for remaining in (0..count).rev() {
            let mut did_skip = false;
            let mut first = true;
            loop {
                if !self.line_empty(curr) {
                    did_skip = true;
                }
                if !first && did_skip && self.starts_ps(curr) {
                    break;
                }
                let next = if forward {
                    (curr < last).then_some(curr + 1)
                } else {
                    curr.checked_sub(1)
                };
                let Some(n) = next else {
                    if remaining > 0 {
                        return false;
                    }
                    break;
                };
                curr = n;
                first = false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fails(text: &str, cursor: usize, motion: Motion, count: u32) -> bool {
        motion_fails(text, cursor, motion, count, |_| Some(1)).is_some()
    }

    #[test]
    fn counted_sentence_motion_fails_past_the_last_sentence() {
        assert!(!fails("uuu vvv www", 10, Motion::SentenceForward, 1));
        assert!(fails("uuu vvv www", 10, Motion::SentenceForward, 2));
        assert!(!fails("k lll mmm", 5, Motion::SentenceBackward, 1));
        assert!(fails("k lll mmm", 5, Motion::SentenceBackward, 2));
        // The step that reaches the end of the buffer does not fail, the
        // one after it does.
        assert!(!fails("aa bb. cc dd.", 0, Motion::SentenceForward, 3));
        assert!(fails("aa bb. cc dd.", 0, Motion::SentenceForward, 4));
    }

    #[test]
    fn counted_paragraph_motion_fails_past_the_last_paragraph() {
        assert!(!fails("one", 0, Motion::ParagraphForward, 1));
        assert!(fails("one", 0, Motion::ParagraphForward, 3));
        // A blank last line ends the paragraph and the next step stops there.
        assert!(!fails("a\n\nb\n", 0, Motion::ParagraphForward, 3));
        assert!(fails("a\nb", 2, Motion::ParagraphBackward, 2));
    }

    #[test]
    fn backward_word_motion_fails_only_from_the_buffer_start() {
        assert!(fails("re", 0, Motion::WordBackward, 1));
        // A step that reaches the start of the buffer ends the motion there.
        assert!(!fails("re", 1, Motion::WordBackward, 2));
        assert!(!fails("e\n1", 2, Motion::WordBackward, 3));
        // An empty first line stops a step, and the next one fails there.
        assert!(motion_fails("\nr", 1, Motion::WordBackward, 2, |_| Some(0)).is_some());
        assert!(motion_fails("\nr", 1, Motion::WordEndBackward, 1, |_| Some(0)).is_none());
    }

    #[test]
    fn line_motions_fail_only_without_moving() {
        assert!(fails("a\nb", 0, Motion::UpFirstNonBlank, 1));
        assert!(!fails("a\nb", 2, Motion::UpFirstNonBlank, 3));
        assert!(fails("a\nb", 2, Motion::DownFirstNonBlank, 1));
        assert!(!fails("a\nb", 2, Motion::FirstNonBlankLine, 1));
        assert!(fails("a\nb", 2, Motion::FirstNonBlankLine, 2));
    }
}
