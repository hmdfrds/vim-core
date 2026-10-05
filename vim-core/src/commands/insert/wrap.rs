//! Breaking lines while typing: a port of Vim 9.1's `internal_format()`.
//!
//! When `textwidth` is set and `formatoptions` has `t` (text) or `c`
//! (comments), typing a non-blank character past the margin breaks the line
//! at the last blank before the cursor. This module decides where the breaks
//! go and what the new lines start with. It ports three pieces of Vim:
//!
//! - the gate in `insertchar()` (edit.c): only a non-blank character
//!   formats, Replace mode formats only when it appends, and the `l` and `b`
//!   flags look at the line the insert started on;
//! - the loop in `internal_format()` (textformat.c): trigger on the cursor's
//!   display column, scan backward from the cursor for a blank, never break
//!   inside the indent or the comment leader, and fall back to the first
//!   blank after a word that is too long;
//! - the parts of `open_line()` (change.c) that build the new line: the
//!   indent with `autoindent` and the continued comment leader with `c`.
//!
//! Vim formats before it inserts the character. Here the typed text is
//! already in the buffer, and the plan lists the breaks as edits to apply
//! after it, in order, so a host that applies effects in sequence ends with
//! the cursor right after the last typed character. A break always lies
//! before the character that caused it, so the typed character itself is
//! never moved out of order or lost.
//!
//! Not ported: `wrapmargin` (the engine does not know the window width, so a
//! `textwidth` of 0 never wraps), the multibyte break flags `m`, `M`, `B`
//! and `]` (text without blanks does not break), numbered lists (`n`,
//! `formatlistpat`), `smartindent` on the new line, and Virtual Replace mode.
//! Text that CTRL-R, CTRL-A or completion inserts in one piece is not
//! formatted, where Vim would type it.

use std::ops::Range;

use compact_str::CompactString;
use unicode_segmentation::UnicodeSegmentation;

use crate::commands::helpers::{column_of, grapheme_display_width, line_of, line_start_for_offset};
use crate::commands::insert::smartindent::build_indent_string;
use crate::effects::{Effect, Effects};
use crate::primitives::{
    CommentSpec, FormatFlags, LeaderMatch, Offset, Range as TextRange, VimOptions,
};
use crate::state::InsertStart;

/// The options that decide whether and how typing breaks a line.
#[derive(Debug, Clone, Copy)]
pub struct FormatPolicy<'a> {
    /// `textwidth`. 0 turns formatting off.
    pub textwidth: usize,
    /// Parsed `formatoptions`.
    pub flags: FormatFlags,
    /// Parsed `comments`.
    pub comments: &'a CommentSpec,
    /// `tabstop`, for display columns and the new indent.
    pub tabstop: usize,
    /// `autoindent`: the new line copies the indent of the broken one.
    pub autoindent: bool,
    /// `expandtab`: the new indent uses spaces only.
    pub expandtab: bool,
}

impl<'a> FormatPolicy<'a> {
    /// The policy for a set of resolved options.
    ///
    /// `wrapmargin` is ignored: Vim derives a width from it and the window
    /// width only when `textwidth` is 0, and the engine has no window.
    #[must_use]
    pub fn from_options(options: &'a VimOptions) -> Self {
        Self {
            textwidth: options.textwidth(),
            flags: options.format_flags(),
            comments: options.comment_spec(),
            tabstop: options.tabstop(),
            autoindent: options.autoindent(),
            expandtab: options.expandtab(),
        }
    }

    /// Whether typing can break a line at all: Vim needs a `textwidth` and
    /// either `t` or `c` in `formatoptions`.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.textwidth > 0
            && self
                .flags
                .intersects(FormatFlags::WRAP_TEXT.union(FormatFlags::WRAP_COMMENTS))
    }
}

/// Text that was just typed, for [`plan_typed_format`].
#[derive(Debug, Clone)]
pub struct TypedRun {
    /// Byte range of the typed text, already in the buffer, typed in order.
    pub range: Range<usize>,
    /// Line index (0-based) of `range.start`.
    pub line: usize,
    /// How many characters at the start of the run overwrote existing text
    /// in Replace mode. Vim does not format while it overwrites.
    pub overwritten: usize,
}

/// One edit of a [`FormatPlan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatEdit {
    /// Byte range replaced, in the buffer as it is when this edit applies.
    pub range: Range<usize>,
    /// Replacement text.
    pub text: CompactString,
}

/// The line breaks formatting adds after some text was typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatPlan {
    /// Edits to apply in order, after the typed text.
    pub edits: Vec<FormatEdit>,
    /// The cursor after all edits: right after the last typed character.
    pub cursor: usize,
}

impl FormatPlan {
    /// The plan for text that starts `base` bytes further into the buffer.
    #[must_use]
    pub fn shifted(mut self, base: usize) -> Self {
        for edit in &mut self.edits {
            edit.range = edit.range.start + base..edit.range.end + base;
        }
        self.cursor += base;
        self
    }
}

/// Plan the breaks Vim makes while `run` is typed.
///
/// `text` holds the typed run in place. Each character of the run is taken
/// in turn, as if typed with the cursor right before it, and goes through
/// Vim's `insertchar()` gate and `internal_format()` loop. `start` is the
/// insert start of the cursor that typed the run, used by the `l`, `v` and
/// `b` flags; without one, the run counts as typed on another line than the
/// insert start. A blank typed on the insert-start line is recorded in it.
///
/// Returns `None` when no line breaks.
#[must_use]
pub fn plan_typed_format(
    text: &str,
    run: &TypedRun,
    policy: &FormatPolicy<'_>,
    mut start: Option<&mut InsertStart>,
) -> Option<FormatPlan> {
    let run_start = run.range.start.min(text.len());
    let run_end = run.range.end.clamp(run_start, text.len());
    let window_start = line_start_for_offset(text, run_start);
    let window_end = text
        .get(run_end..)
        .and_then(|rest| rest.find('\n'))
        .map_or(text.len(), |n| run_end + n);
    let mut buf = text.get(window_start..window_end)?.to_owned();

    let mut state = Formatter {
        policy,
        edits: Vec::new(),
        run_end: run_end - window_start,
    };
    let mut pos = run_start - window_start;
    let mut line = run.line;
    let mut index = 0;
    while pos < state.run_end {
        let Some(c) = buf.get(pos..).and_then(|rest| rest.chars().next()) else {
            break;
        };
        if c == '\n' {
            line += 1;
        } else {
            if is_blank(c) {
                if let Some(s) = start.as_deref_mut() {
                    if s.blank_vcol.is_none() && s.line == line {
                        let ls = line_start_for_offset(&buf, pos);
                        s.blank_vcol = Some(display_width(buf.get(ls..pos)?, policy.tabstop));
                    }
                }
            }
            if may_format(c, index < run.overwritten, line, start.as_deref(), policy) {
                pos = state.internal_format(&mut buf, pos, c, &mut line, start.as_deref());
            }
        }
        pos += c.len_utf8();
        index += 1;
    }

    if state.edits.is_empty() {
        return None;
    }
    let edits = state
        .edits
        .into_iter()
        .map(|e| FormatEdit {
            range: e.range.start + window_start..e.range.end + window_start,
            text: e.text,
        })
        .collect();
    Some(FormatPlan {
        edits,
        cursor: state.run_end + window_start,
    })
}

/// The insert start for a cursor at `cursor` in `text`.
///
/// `text` is the buffer before the insert changes anything. `blank_vcol`
/// carries over from an earlier record, as Vim keeps `Insstart_blank_vcol`
/// when `stop_arrow()` moves the start.
#[must_use]
pub fn insert_start_at(
    text: &str,
    cursor: usize,
    tabstop: usize,
    blank_vcol: Option<usize>,
) -> InsertStart {
    let cursor = cursor.min(text.len());
    let ls = line_start_for_offset(text, cursor);
    let le = text
        .get(cursor..)
        .and_then(|rest| rest.find('\n'))
        .map_or(text.len(), |n| cursor + n);
    InsertStart {
        line: line_of(text, cursor),
        col: column_of(text, cursor),
        textlen: text.get(ls..le).map_or(0, |l| display_width(l, tabstop)),
        blank_vcol,
    }
}

/// Format the character an insert command typed at `cursor` and splice
/// the line breaks into the command's `effects`.
///
/// `text` is the buffer before the command. The typed character is the one
/// right before the cursor the effects end with. `replace_mode` is Replace
/// mode, where only a character appended at the end of the line formats.
/// Returns whether a line was broken.
pub fn format_typed_char(
    effects: &mut Effects,
    text: &str,
    cursor: usize,
    replace_mode: bool,
    policy: &FormatPolicy<'_>,
    start: Option<&mut InsertStart>,
) -> bool {
    let Some((window, base)) = line_after_effects(text, cursor, effects.as_slice()) else {
        return false;
    };
    let Some(end) = last_cursor(effects.as_slice())
        .and_then(|c| c.checked_sub(base))
        .filter(|&end| end <= window.len())
    else {
        return false;
    };
    let Some(c) = window.get(..end).and_then(|s| s.chars().next_back()) else {
        return false;
    };
    if c == '\n' {
        return false;
    }
    let overwrites = replace_mode
        && text
            .get(cursor..)
            .and_then(|rest| rest.chars().next())
            .is_some_and(|next| next != '\n');
    let run = TypedRun {
        range: end - c.len_utf8()..end,
        line: line_of(text, cursor),
        overwritten: usize::from(overwrites),
    };
    let Some(plan) = plan_typed_format(&window, &run, policy, start) else {
        return false;
    };
    splice_format_plan(effects, &plan.shifted(base));
    true
}

/// Format text that a repeat inserts in one piece, a dot-repeat or the count
/// of an insert, and splice the line breaks into `effects`. Vim replays the
/// text as typed, so it breaks the same way.
///
/// `earlier` are edits applied to `text` before `effects`, and `typed` is
/// the range of the inserted text once `effects` are applied. `start` is
/// the insert start to use; without one, the insert starts where the text
/// goes, as for a new insert. Returns where the cursor ends after the last
/// typed character, or `None` when no line broke.
pub fn format_inserted_text(
    effects: &mut Effects,
    text: &str,
    earlier: &[Effect],
    typed: Range<usize>,
    policy: &FormatPolicy<'_>,
    start: Option<InsertStart>,
) -> Option<usize> {
    if !policy.is_active() {
        return None;
    }
    let before = apply_text_effects(text, earlier)?;
    let after = apply_text_effects(&before, effects.as_slice())?;
    let mut start =
        start.unwrap_or_else(|| insert_start_at(&before, typed.start, policy.tabstop, None));
    let run = TypedRun {
        line: line_of(&after, typed.start),
        range: typed,
        overwritten: 0,
    };
    let plan = plan_typed_format(&after, &run, policy, Some(&mut start))?;
    splice_format_plan(effects, &plan);
    Some(plan.cursor)
}

/// Splice the edits of `plan` into `effects` right before the last
/// `SetCursor`, and move that cursor past the edits. Without a `SetCursor`
/// the edits go at the end.
///
/// Every formatting path goes through here, so the breaks always land in
/// order after the typed text and before the final cursor, which keeps the
/// "`SetCursor` follows all edits" invariant.
pub fn splice_format_plan(effects: &mut Effects, plan: &FormatPlan) {
    let mut list: Vec<Effect> = effects.drain().collect();
    let cursor_index = list
        .iter()
        .rposition(|e| matches!(e, Effect::SetCursor { .. }));
    let cursor = match cursor_index.and_then(|i| list.get(i)) {
        Some(Effect::SetCursor { offset }) => Some(map_offset(offset.get(), &plan.edits)),
        _ => None,
    };
    let at = cursor_index.unwrap_or(list.len());
    let edits = plan.edits.iter().map(|e| {
        let range = TextRange::new(Offset::new(e.range.start), Offset::new(e.range.end));
        if e.text.is_empty() {
            Effect::Delete { range }
        } else if e.range.is_empty() {
            Effect::Insert {
                offset: range.start(),
                text: e.text.clone(),
            }
        } else {
            Effect::Replace {
                range,
                text: e.text.clone(),
            }
        }
    });
    list.splice(at..at, edits);
    if let (Some(cursor), Some(slot)) = (cursor, list.get_mut(at + plan.edits.len())) {
        *slot = Effect::SetCursor {
            offset: Offset::new(cursor),
        };
    }
    // A bracket flash after the edits points into the text as it was.
    for effect in list.iter_mut().skip(at + plan.edits.len()) {
        if let Effect::ShowMatch { position } = effect {
            *position = Offset::new(map_offset(position.get(), &plan.edits));
        }
    }
    debug_assert!(
        crate::effects::validate_ordering(&list).is_ok(),
        "formatting edits must come before the final SetCursor"
    );
    effects.extend(list);
}

/// Where `offset` ends up after `edits` are applied in order.
fn map_offset(mut offset: usize, edits: &[FormatEdit]) -> usize {
    for edit in edits {
        if offset >= edit.range.end {
            offset = offset - edit.range.len() + edit.text.len();
        } else if offset > edit.range.start {
            offset = edit.range.start;
        }
    }
    offset
}

/// The line holding `cursor`, after the text edits in `effects`.
///
/// Returns the line and the offset in `text` where it starts, or `None` when
/// an edit reaches outside the line.
#[must_use]
pub fn line_after_effects(
    text: &str,
    cursor: usize,
    effects: &[Effect],
) -> Option<(String, usize)> {
    let cursor = cursor.min(text.len());
    let base = line_start_for_offset(text, cursor);
    let end = text
        .get(cursor..)
        .and_then(|rest| rest.find('\n'))
        .map_or(text.len(), |n| cursor + n);
    let mut line = text.get(base..end)?.to_owned();
    for effect in effects {
        let (range, insert) = match effect {
            Effect::Insert { offset, text } => (offset.get()..offset.get(), text.as_str()),
            Effect::Delete { range } => (range.start().get()..range.end().get(), ""),
            Effect::Replace { range, text } => {
                (range.start().get()..range.end().get(), text.as_str())
            }
            _ => continue,
        };
        let local = range.start.checked_sub(base)?..range.end.checked_sub(base)?;
        if local.start > local.end || local.end > line.len() {
            return None;
        }
        line.get(local.clone())?;
        line.replace_range(local, insert);
    }
    Some((line, base))
}

/// `text` after the text edits in `effects`, applied in order, or `None`
/// when an edit does not fit.
#[must_use]
pub fn apply_text_effects(text: &str, effects: &[Effect]) -> Option<String> {
    let mut out = text.to_owned();
    for effect in effects {
        let (range, insert) = match effect {
            Effect::Insert { offset, text } => (offset.get()..offset.get(), text.as_str()),
            Effect::Delete { range } => (range.start().get()..range.end().get(), ""),
            Effect::Replace { range, text } => {
                (range.start().get()..range.end().get(), text.as_str())
            }
            _ => continue,
        };
        out.get(range.clone())?;
        out.replace_range(range, insert);
    }
    Some(out)
}

/// The offset of the last `SetCursor` in `effects`.
fn last_cursor(effects: &[Effect]) -> Option<usize> {
    effects.iter().rev().find_map(|e| match e {
        Effect::SetCursor { offset } => Some(offset.get()),
        _ => None,
    })
}

/// The gate in Vim's `insertchar()`: whether typing `c` may format.
fn may_format(
    c: char,
    overwrites: bool,
    line: usize,
    start: Option<&InsertStart>,
    policy: &FormatPolicy<'_>,
) -> bool {
    if !policy.is_active() || is_blank(c) || overwrites {
        return false;
    }
    let tw = policy.textwidth;
    match start {
        Some(s) if s.line == line => {
            (!policy.flags.contains(FormatFlags::LONG_LINES) || s.textlen <= tw)
                && (!policy.flags.contains(FormatFlags::BLANK_WRAP)
                    || s.blank_vcol.is_some_and(|v| v <= tw))
        }
        _ => true,
    }
}

/// Working state while one run is formatted.
struct Formatter<'p, 'a> {
    policy: &'p FormatPolicy<'a>,
    edits: Vec<FormatEdit>,
    /// End of the typed run in the window, kept up to date as lines break.
    run_end: usize,
}

impl Formatter<'_, '_> {
    /// Port of Vim's `internal_format()` for the character `c` at `pos`.
    ///
    /// Breaks the line until the cursor fits, records each break, and
    /// returns the new position of `c`.
    fn internal_format(
        &mut self,
        buf: &mut String,
        mut pos: usize,
        c: char,
        line: &mut usize,
        start: Option<&InsertStart>,
    ) -> usize {
        let policy = self.policy;
        let flags = policy.flags;
        let tw = policy.textwidth;
        let c_len = c.len_utf8();
        let mut no_leader = false;

        loop {
            let ls = line_start_for_offset(buf, pos);
            let le = buf
                .get(pos..)
                .and_then(|rest| rest.find('\n'))
                .map_or(buf.len(), |n| pos + n);
            let (Some(before), Some(after), Some(line1)) =
                (buf.get(ls..pos), buf.get(pos + c_len..le), buf.get(ls..le))
            else {
                break;
            };
            let startcol = pos - ls;
            let mut c_buf = [0u8; 4];
            let virtcol = display_width(before, policy.tabstop);
            let cells = grapheme_display_width(c.encode_utf8(&mut c_buf), virtcol, policy.tabstop);
            if virtcol + cells <= tw {
                break;
            }

            // Vim formats before inserting the character, so the comment
            // leader is matched on the line without it.
            let line0 = format!("{before}{after}");
            let do_comments = !no_leader && flags.contains(FormatFlags::WRAP_COMMENTS);
            let leader = if do_comments {
                policy.comments.match_line(&line0)
            } else {
                None
            };
            let leader_len = leader.map_or(0, |m| m.ws_end);
            // A line that does not start with a leader must not give one to
            // the lines broken off it.
            if leader_len == 0 {
                no_leader = true;
                if !flags.contains(FormatFlags::WRAP_TEXT) {
                    break;
                }
            }
            if startcol == 0 {
                break;
            }

            let wantcol = column_at(line1, tw, policy.tabstop);
            let restrict_col = start.filter(|s| s.line == *line).map(|s| s.col);
            let Some(foundcol) =
                find_break(line1, startcol, wantcol, leader_len, restrict_col, flags)
            else {
                break;
            };

            // Skip the blanks at the break. They are deleted unless `w`
            // keeps them to mark the paragraph as continuing.
            let white_par = flags.contains(FormatFlags::WHITE_PARAGRAPH);
            let mut col = foundcol;
            while byte_at(line1, col).is_some_and(is_blank_byte) && (!white_par || col < startcol) {
                col += 1;
            }
            let split = if white_par { col } else { foundcol };
            let cursor_offset = startcol.saturating_sub(col);

            // With 'autoindent' off Vim protects the blank under the cursor
            // from the deletion of blanks at the break. With it on, blanks
            // that directly follow the typed text are deleted as well.
            let trailing_blanks = if policy.autoindent && col == startcol {
                self.blanks_after_run(buf, pos)
            } else {
                0
            };

            let new_line = self.new_line_text(&line0, leader, split);
            if trailing_blanks > 0 {
                let range = self.run_end..self.run_end + trailing_blanks;
                buf.replace_range(range.clone(), "");
                self.edits.push(FormatEdit {
                    range,
                    text: CompactString::default(),
                });
            }
            let range = ls + split..ls + col;
            let removed = range.len();
            buf.replace_range(range.clone(), &new_line);
            let new_pos = ls + split + new_line.len() + cursor_offset;
            self.run_end = self.run_end + new_line.len() - removed;
            self.edits.push(FormatEdit {
                range,
                text: new_line,
            });
            pos = new_pos;
            *line += 1;
        }
        pos
    }

    /// Blanks right after the typed run, on the cursor line.
    fn blanks_after_run(&self, buf: &str, pos: usize) -> usize {
        if buf
            .get(pos..self.run_end)
            .is_none_or(|typed| typed.contains('\n'))
        {
            return 0;
        }
        buf.get(self.run_end..).map_or(0, |rest| {
            rest.bytes().take_while(|&b| is_blank_byte(b)).count()
        })
    }

    /// The text that replaces the blanks at a break: a newline, the new
    /// indent and, on a comment line with `c`, the continued leader.
    fn new_line_text(
        &self,
        line0: &str,
        leader: Option<LeaderMatch>,
        split: usize,
    ) -> CompactString {
        let policy = self.policy;
        let continuation = leader.and_then(|m| {
            m.continuation(
                policy.comments,
                line0,
                split,
                policy.autoindent,
                policy.tabstop,
            )
        });
        let (indent, leader_text) = match continuation {
            Some(cont) => (cont.indent, cont.leader),
            None if policy.autoindent => (
                indent_width(line0, policy.tabstop),
                CompactString::default(),
            ),
            None => (0, CompactString::default()),
        };
        let mut text = CompactString::from("\n");
        text.push_str(&build_indent_string(
            indent,
            !policy.expandtab,
            policy.tabstop,
        ));
        text.push_str(&leader_text);
        text
    }
}

/// The scan in Vim's `internal_format()`: walk back from the cursor to the
/// blank to break at. Returns the column of the first blank of the run of
/// blanks found, which the line is split at.
///
/// The first blank whose position fits in `textwidth` wins. A long word
/// leaves the last blank found, the leftmost one, which breaks after the
/// word. Positions inside the indent or the comment leader are never used.
/// With `v` or `b` on the insert-start line, only blanks at or after the
/// insert-start column (`restrict_col`) count.
fn find_break(
    line: &str,
    startcol: usize,
    wantcol: usize,
    leader_len: usize,
    restrict_col: Option<usize>,
    flags: FormatFlags,
) -> Option<usize> {
    let vi_scan = flags.intersects(FormatFlags::VI_WRAP.union(FormatFlags::BLANK_WRAP));
    let one_letter = flags.contains(FormatFlags::ONE_LETTER);
    let period = flags.contains(FormatFlags::PERIOD_ABBREVIATION);
    let blank_at = |col: usize| byte_at(line, col).is_some_and(is_blank_byte);

    let mut col = startcol;
    let mut foundcol = 0;
    loop {
        if vi_scan && restrict_col.is_some_and(|rc| col < rc) {
            break;
        }
        if blank_at(col) {
            // Find the start of this run of blanks.
            let mut blanks = 0;
            while col > 0 && blank_at(col) {
                col = prev_char(line, col);
                blanks = (blanks + 1).min(2);
            }
            if col == 0 && blank_at(col) {
                break; // only blanks before the text
            }
            // `p`: do not break after a period followed by a single space.
            if period && byte_at(line, col) == Some(b'.') && blanks < 2 {
                continue;
            }
            if col < leader_len {
                break;
            }
            if one_letter {
                // `1`: do not break after a one-letter word.
                if col == 0 || col <= leader_len {
                    break;
                }
                let word_end = col;
                col = prev_char(line, col);
                if blank_at(col) {
                    continue;
                }
                col = word_end;
            }
            col = next_char(line, col);
            foundcol = col;
            if col <= wantcol {
                break;
            }
        }
        if col == 0 {
            break;
        }
        col = prev_char(line, col);
    }
    (foundcol > 0).then_some(foundcol)
}

/// Byte column of the character covering display column `vcol`, or the end
/// of the line (Vim's `coladvance()` in Insert mode).
fn column_at(line: &str, vcol: usize, tabstop: usize) -> usize {
    let mut col = 0;
    for (i, g) in line.grapheme_indices(true) {
        let w = grapheme_display_width(g, col, tabstop);
        if col + w > vcol {
            return i;
        }
        col += w;
    }
    line.len()
}

/// Display width of `s`, which starts at a line start.
fn display_width(s: &str, tabstop: usize) -> usize {
    s.graphemes(true)
        .fold(0, |col, g| col + grapheme_display_width(g, col, tabstop))
}

/// Width of the leading white space of `line` in display columns.
fn indent_width(line: &str, tabstop: usize) -> usize {
    let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    line.get(..indent_len)
        .map_or(0, |indent| display_width(indent, tabstop))
}

fn prev_char(line: &str, col: usize) -> usize {
    line.get(..col)
        .and_then(|s| s.char_indices().next_back())
        .map_or(0, |(i, _)| i)
}

fn next_char(line: &str, col: usize) -> usize {
    line.get(col..)
        .and_then(|s| s.chars().next())
        .map_or(col, |c| col + c.len_utf8())
}

fn byte_at(line: &str, col: usize) -> Option<u8> {
    line.as_bytes().get(col).copied()
}

/// Vim's `ascii_iswhite()`.
const fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

const fn is_blank_byte(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::DEFAULT_COMMENTS;

    fn policy<'a>(
        spec: &'a CommentSpec,
        tw: usize,
        fo: &str,
        ts: usize,
        ai: bool,
    ) -> FormatPolicy<'a> {
        FormatPolicy {
            textwidth: tw,
            flags: FormatFlags::parse(fo).unwrap(),
            comments: spec,
            tabstop: ts,
            autoindent: ai,
            expandtab: false,
        }
    }

    fn apply(text: &mut String, edits: &[FormatEdit]) {
        for edit in edits {
            text.replace_range(edit.range.clone(), &edit.text);
        }
    }

    /// Type `typed` one character at a time at the `|` in `before`, the
    /// way the engine does, and return the result with `|` at the cursor.
    fn type_at(before: &str, typed: &str, policy: &FormatPolicy<'_>) -> String {
        let mut cursor = before.find('|').unwrap();
        let mut text = before.replacen('|', "", 1);
        let mut start = Some(insert_start_at(&text, cursor, policy.tabstop, None));
        for c in typed.chars() {
            text.insert(cursor, c);
            let end = cursor + c.len_utf8();
            let run = TypedRun {
                range: cursor..end,
                line: line_of(&text, cursor),
                overwritten: 0,
            };
            cursor = match plan_typed_format(&text, &run, policy, start.as_mut()) {
                Some(plan) => {
                    apply(&mut text, &plan.edits);
                    plan.cursor
                }
                None => end,
            };
        }
        text.insert(cursor, '|');
        text
    }

    /// Expected results recorded from headless Vim 9.1 (`:normal!` with
    /// explicit options, `sw` equal to `ts`).
    #[test]
    fn breaks_like_vim() {
        #[rustfmt::skip]
        let table: &[(&str, &str, &str, usize, &str, usize, bool, &str, &str)] = &[
            // (name, before, typed, tw, fo, ts, ai, comments, after)
            ("tab indent ts=4", "\t|", "foo bar baz qux quux", 20, "tq", 4, true, DEFAULT_COMMENTS, "\tfoo bar baz qux\n\tquux|"),
            ("tab indent ts=8", "\t|", "foo bar baz qux quux", 20, "tq", 8, true, DEFAULT_COMMENTS, "\tfoo bar baz\n\tqux quux|"),
            ("space indent", "    |", "foo bar baz qux quux", 20, "tq", 8, true, DEFAULT_COMMENTS, "    foo bar baz qux\n    quux|"),
            ("space indent without autoindent", "    |", "foo bar baz qux quux", 20, "tq", 8, false, DEFAULT_COMMENTS, "    foo bar baz qux\nquux|"),
            ("cjk", "|", "日本語 日本語 日本語", 10, "tq", 8, false, DEFAULT_COMMENTS, "日本語\n日本語\n日本語|"),
            ("cursor left of textwidth", "a|a bbbb cccc dddd eeee ffff", "xy", 20, "tq", 8, false, DEFAULT_COMMENTS, "axy|a bbbb cccc dddd eeee ffff"),
            ("indent-only line", "            |", "abc def", 10, "tq", 8, false, DEFAULT_COMMENTS, "            abc\ndef|"),
            ("long word", "|", "a verylongword b", 10, "tq", 8, false, DEFAULT_COMMENTS, "a\nverylongword\nb|"),
            ("l keeps a long line", "aaaa bbbb cccc dddd eeee|", " ffff", 20, "tql", 8, false, DEFAULT_COMMENTS, "aaaa bbbb cccc dddd eeee ffff|"),
            ("v breaks at a typed blank", "aaaa bbbb cccc dddd|", " eeee", 20, "tqv", 8, false, DEFAULT_COMMENTS, "aaaa bbbb cccc dddd\neeee|"),
            ("v ignores older blanks", "aaaa bbbb cccc dddd|", "eeee", 20, "tqv", 8, false, DEFAULT_COMMENTS, "aaaa bbbb cccc ddddeeee|"),
            ("b needs a blank before the margin", "aaaa bbbb cccc dddd eee|", " ffff", 20, "tqb", 8, false, DEFAULT_COMMENTS, "aaaa bbbb cccc dddd eee ffff|"),
            ("c without t leaves text", "|", "aaaa bbbb cccc dddd eeee", 20, "cq", 8, false, DEFAULT_COMMENTS, "aaaa bbbb cccc dddd eeee|"),
            ("c without t wraps a comment", "|", "# aaaa bbbb cccc dddd eeee", 20, "cq", 8, false, DEFAULT_COMMENTS, "# aaaa bbbb cccc\n# dddd eeee|"),
            ("hash leader", "    |", "# aaaa bbbb cccc dddd eeee", 20, "cq", 8, true, DEFAULT_COMMENTS, "    # aaaa bbbb cccc\n    # dddd eeee|"),
            ("double hash leader", "|", "## aaaa bbbb cccc dddd eeee", 20, "cq", 8, false, "b:##,b:#", "## aaaa bbbb cccc\n## dddd eeee|"),
            ("three-part comment", "|", "/* aaaa bbbb cccc dddd eeee", 20, "cq", 8, false, DEFAULT_COMMENTS, "/* aaaa bbbb cccc\n * dddd eeee|"),
            ("three-part middle", " * |", "aaaa bbbb cccc dddd eeee", 20, "cq", 8, false, DEFAULT_COMMENTS, " * aaaa bbbb cccc\n * dddd eeee|"),
        ];
        for &(name, before, typed, tw, fo, ts, ai, comments, after) in table {
            let spec = CommentSpec::parse(comments).unwrap();
            let policy = policy(&spec, tw, fo, ts, ai);
            assert_eq!(type_at(before, typed, &policy), after, "{name}");
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(512))]

        /// Each break only turns blanks into a newline, an indent and a
        /// leader: undoing the edits of a plan gives back the text with just
        /// the typed character, and the cursor ends right after it.
        #[test]
        fn breaks_only_replace_blanks(
            words in proptest::collection::vec("[a-z]{1,8}|日本|\t", 0..8),
            prefix in proptest::sample::select(vec!["", "# ", "## ", "    ", "\t", " * ", "/* ", "- "]),
            typed in "[a-z日# ]{1,30}",
            tw in 1usize..30,
            fo in proptest::sample::select(vec!["tq", "cq", "tcq", "tcqw", "tq1", "tqp", "tql", "tqv", "tqb"]),
            ai in proptest::bool::ANY,
            ts in proptest::sample::select(vec![2usize, 4, 8]),
            cursor_pick in 0usize..100,
        ) {
            let spec = CommentSpec::parse(DEFAULT_COMMENTS).unwrap();
            let policy = policy(&spec, tw, fo, ts, ai);
            let mut text = format!("{prefix}{}", words.join(" "));
            let boundaries: Vec<usize> =
                text.char_indices().map(|(i, _)| i).chain([text.len()]).collect();
            let mut cursor = boundaries[cursor_pick % boundaries.len()];
            let mut start = Some(insert_start_at(&text, cursor, ts, None));
            for c in typed.chars() {
                text.insert(cursor, c);
                let end = cursor + c.len_utf8();
                let typed_text = text.clone();
                let run = TypedRun {
                    range: cursor..end,
                    line: line_of(&text, cursor),
                    overwritten: 0,
                };
                let Some(plan) = plan_typed_format(&text, &run, &policy, start.as_mut()) else {
                    cursor = end;
                    continue;
                };
                let mut undo = Vec::new();
                for edit in &plan.edits {
                    let removed = text[edit.range.clone()].to_owned();
                    proptest::prop_assert!(removed.bytes().all(is_blank_byte), "{removed:?}");
                    proptest::prop_assert!(
                        edit.text.is_empty() || edit.text.starts_with('\n'),
                        "{:?}",
                        edit.text
                    );
                    text.replace_range(edit.range.clone(), &edit.text);
                    undo.push((edit.range.start..edit.range.start + edit.text.len(), removed));
                }
                proptest::prop_assert!(text[..plan.cursor].ends_with(c));
                let mut reverted = text.clone();
                for (range, removed) in undo.iter().rev() {
                    reverted.replace_range(range.clone(), removed);
                }
                proptest::prop_assert_eq!(&reverted, &typed_text);
                cursor = plan.cursor;
            }
        }
    }

    #[test]
    fn breaks_lie_before_the_typed_character() {
        let spec = CommentSpec::parse(DEFAULT_COMMENTS).unwrap();
        let policy = policy(&spec, 10, "tq", 8, false);
        let text = "aaaa bbbbbbx";
        let run = TypedRun {
            range: 11..12,
            line: 0,
            overwritten: 0,
        };
        let plan = plan_typed_format(text, &run, &policy, None).unwrap();
        assert_eq!(
            plan.edits,
            vec![FormatEdit {
                range: 4..5,
                text: "\n".into(),
            }]
        );
        assert_eq!(plan.cursor, 12);
    }

    #[test]
    fn replace_mode_overwrite_does_not_format() {
        let spec = CommentSpec::parse(DEFAULT_COMMENTS).unwrap();
        let policy = policy(&spec, 10, "tq", 8, false);
        let run = TypedRun {
            range: 11..12,
            line: 0,
            overwritten: 1,
        };
        assert_eq!(plan_typed_format("aaaa bbbbbbx", &run, &policy, None), None);
    }

    #[test]
    fn blank_typed_on_start_line_is_recorded() {
        let spec = CommentSpec::parse(DEFAULT_COMMENTS).unwrap();
        let policy = policy(&spec, 20, "tqb", 4, false);
        let mut start = insert_start_at("\tab", 3, 4, None);
        let run = TypedRun {
            range: 3..4,
            line: 0,
            overwritten: 0,
        };
        assert_eq!(
            plan_typed_format("\tab ", &run, &policy, Some(&mut start)),
            None
        );
        assert_eq!(start.blank_vcol, Some(6));
        assert_eq!(start.textlen, 6);
    }

    #[test]
    fn textwidth_zero_never_wraps_even_with_wrapmargin() {
        // 'wrapmargin' is not ported: without a window the engine cannot
        // turn it into a width, so textwidth 0 always means no wrapping.
        let mut options = VimOptions::default();
        options.set_textwidth(0);
        options.set_wrapmargin(10);
        options.set_formatoptions("tcq");
        assert!(!FormatPolicy::from_options(&options).is_active());
        options.set_textwidth(20);
        assert!(FormatPolicy::from_options(&options).is_active());
        options.set_formatoptions("q");
        assert!(!FormatPolicy::from_options(&options).is_active());
    }

    #[test]
    fn splice_puts_edits_before_the_final_cursor() {
        let mut effects = Effects::new()
            .insert(Offset::new(11), "x")
            .set_cursor(Offset::new(12));
        let plan = FormatPlan {
            edits: vec![FormatEdit {
                range: 4..5,
                text: "\n  ".into(),
            }],
            cursor: 14,
        };
        splice_format_plan(&mut effects, &plan);
        assert_eq!(
            effects.as_slice(),
            &[
                Effect::Insert {
                    offset: Offset::new(11),
                    text: "x".into(),
                },
                Effect::Replace {
                    range: TextRange::new(Offset::new(4), Offset::new(5)),
                    text: "\n  ".into(),
                },
                Effect::SetCursor {
                    offset: Offset::new(14),
                },
            ]
        );
    }

    #[test]
    fn line_after_effects_refuses_edits_off_the_line() {
        let effects = [Effect::Insert {
            offset: Offset::new(0),
            text: "x".into(),
        }];
        assert_eq!(
            line_after_effects("ab\ncd", 4, &effects),
            None,
            "an edit on another line"
        );
        assert_eq!(
            line_after_effects("ab\ncd", 1, &effects),
            Some(("xab".to_owned(), 0))
        );
    }
}
