//! Format operator (gq, gw).
//!
//! A port of Vim 9.1's `op_format()` and `format_lines()` (textformat.c).
//! The lines of the range are split into paragraphs, the lines of each
//! paragraph are joined, and the joined line is broken again at the margin
//! by the same `internal_format()` port that breaks lines while typing
//! ([`crate::commands::insert::wrap`]).
//!
//! # Behavior
//!
//! - `gqq` formats the current line, `gqap` a paragraph. `gw` formats the
//!   same way and leaves the cursor on the same text.
//! - The operator always works on whole lines. `textwidth` 0 formats at 79
//!   columns.
//! - A paragraph ends at an empty or white-space-only line, at a line that
//!   starts with a form feed or an nroff macro, and with `q` at a change of
//!   comment leader, at a line holding only a leader and at the end part of
//!   a three-piece comment. Those lines stay as they are.
//! - With `q` the leader of every joined line is removed and every broken
//!   line gets one back, so a `#` block reflows inside the comment.
//! - The first line of a paragraph gets its indent rebuilt with `tabstop`
//!   and `expandtab`; the broken lines copy it when `autoindent` is set.
//!   `2` takes the indent from the second line of the paragraph.
//! - `w` makes a line that ends in white space continue the paragraph.
//!
//! Not ported: `formatexpr` and `formatprg`, numbered lists (`n`),
//! `joinspaces` (the engine has no such option and uses Neovim's default,
//! off), and the C and Lisp indent Vim gives a new paragraph with `cindent`
//! or `lisp`.

use super::types::{OperatorContext, OperatorOrigin};
use crate::commands::helpers::{line_end_for_offset, line_start_for_offset, prev_char_boundary};
use crate::commands::insert::smartindent::build_indent_string;
use crate::commands::insert::wrap::{
    format_line_for_operator, map_offset, FormatPolicy, OperatorCall,
};
use crate::commands::CommandResult;
use crate::effects::Effects;
use crate::primitives::{
    CommentFlags, CommentPart, CommentSpec, FormatFlags, LeaderMatch, MarkName, Offset, Range,
    VimOptions,
};
use compact_str::CompactString;
use unicode_segmentation::UnicodeSegmentation;

/// Width `gq` formats at when `textwidth` is 0 (Vim's `comp_textwidth()`
/// with a window of at least 80 columns).
const DEFAULT_FORMAT_WIDTH: usize = 79;

/// The `textwidth` the Visual format operators used before they read the
/// option.
const VISUAL_WIDTH_BEFORE: usize = 80;

/// Vim's default `paragraphs`: nroff macros that start a paragraph.
const PARAGRAPH_MACROS: &str = "IPLPPPQPP TPHPLIPpLpItpplpipbp";

/// Vim's default `sections`: nroff macros that start a section.
const SECTION_MACROS: &str = "SHNHH HUnhsh";

/// Execute format operator (gq).
///
/// No traits, just functions + enum dispatch.
pub fn execute(ctx: &OperatorContext<'_>) -> CommandResult {
    if from_empty_last_line(ctx) {
        return super::format_legacy::execute(ctx);
    }
    if ctx.is_empty() {
        // Neovim always sets `[` and `]` marks even on empty buffer gq.
        let effects = Effects::new()
            .set_mark(MarkName::CHANGE_START, ctx.cursor, None)
            .set_mark(MarkName::CHANGE_END, ctx.cursor, None)
            .set_cursor(ctx.cursor);
        return CommandResult::new(effects, ctx.cursor);
    }

    let formatted = format_range(ctx, None);
    let new_text = formatted.new_text(ctx.text);

    // Vim leaves the cursor on the first non-blank of the last line it
    // formatted. When the motion was exclusive and ended in column 0 (`gq}`)
    // Vim formatted up to the line before and moves on to that line, so
    // that `.` formats the next paragraph.
    let mut cursor_line = formatted.first_line + formatted.last_line;
    if formatted.end_adjusted && cursor_line < line_count(&new_text) - 1 {
        cursor_line += 1;
    }
    // A count that takes `gqq` onto the empty line after a final newline
    // formats that line too, and Vim leaves the cursor on it.
    if ctx.origin == OperatorOrigin::TextObject
        && ctx.motion_target.get() == ctx.text.len()
        && ctx.motion_target > ctx.cursor
        && ctx.text.ends_with('\n')
    {
        cursor_line = line_count(&new_text) - 1;
    }
    let new_cursor = Offset::new(begin_line(&new_text, cursor_line));

    // Neovim's gq sets `[` = start of range, `]` = cursor position after
    // formatting (b_op_end = curwin->w_cursor from op_format).
    let mark_start = Offset::new(formatted.range_start);
    if !formatted.changed() {
        return CommandResult::new(
            undo_step_without_change(&formatted)
                .set_mark(MarkName::CHANGE_START, mark_start, None)
                .set_mark(MarkName::CHANGE_END, new_cursor, None)
                .set_cursor(new_cursor)
                .end_undo(),
            new_cursor,
        );
    }

    // Mark `.` = just past the last formatted line that has text, on the
    // start of the next line when there is one. The Replace effect's
    // sync_change_marks sets mark `.` to the range start, so it is set
    // explicitly here.
    let mark_dot = Offset::new(formatted.change_end());
    let effects = Effects::new()
        .begin_undo_force_entry()
        .replace(
            formatted.replaced_range(),
            CompactString::new(&formatted.text),
        )
        .set_mark(MarkName::CHANGE_START, mark_start, None)
        .set_mark(MarkName::CHANGE_END, new_cursor, None)
        .set_mark(MarkName::LAST_CHANGE, mark_dot, None)
        .set_cursor(new_cursor)
        .end_undo();
    CommandResult::new(effects, new_cursor)
}

/// Whether the format operators format the lines that a text object of
/// `kind` at `cursor` touches, as Vim does.
///
/// The word and sentence objects select differently than Vim from white
/// space and blank lines, apart from a word object on an empty line, and
/// the kinds left out are not Vim's, so Vim refuses them. On those the
/// operators wrap the selected text as they did before (see
/// [`execute_as_before`]).
#[must_use]
pub fn formats_text_object(
    kind: crate::grammar::types::TextObjectKind,
    text: &str,
    cursor: usize,
) -> bool {
    use crate::grammar::types::TextObjectKind as K;
    match kind {
        K::Paragraph
        | K::Paren
        | K::Brace
        | K::Bracket
        | K::Angle
        | K::DoubleQuote
        | K::SingleQuote
        | K::Backtick
        | K::Tag => true,
        // An empty line is a word in Vim. Formatting the lines the engine's
        // object covers from it gives Vim's text; the cursor can still end
        // on another of those lines than in Vim.
        K::Word | K::WORD if on_empty_line(text, cursor) => true,
        K::Word | K::WORD | K::Sentence => text
            .get(cursor..)
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| !c.is_whitespace()),
        _ => false,
    }
}

/// Whether `cursor` is on an empty line.
fn on_empty_line(text: &str, cursor: usize) -> bool {
    let bytes = text.as_bytes();
    let at_start = cursor == 0 || bytes.get(cursor - 1) == Some(&b'\n');
    let at_end = cursor >= bytes.len() || bytes.get(cursor) == Some(&b'\n');
    at_start && at_end
}

/// The format operators as they were before they formatted whole lines.
///
/// The text of the range is wrapped at `textwidth`, without comment leaders
/// or `formatoptions`. `keep_cursor` is `gw`.
pub fn execute_as_before(ctx: &OperatorContext<'_>, keep_cursor: bool) -> CommandResult {
    if keep_cursor {
        super::format_legacy::execute_keep_cursor(ctx)
    } else {
        super::format_legacy::execute(ctx)
    }
}

/// The result of `gq` or `gw` when Vim cancels it because `motion` fails.
///
/// The engine's motion stops at the edge of the buffer where Vim's fails
/// with `count` from `cursor`. `None` when the motion does not fail.
/// `target(n)` is where the engine's motion goes with count `n`.
pub fn cancel_for_failed_motion(
    text: &str,
    cursor: usize,
    motion: crate::grammar::types::Motion,
    count: u32,
    target: impl Fn(u32) -> Option<usize>,
) -> Option<CommandResult> {
    use super::format_cancel::{motion_fails, Cancel};
    Some(match motion_fails(text, cursor, motion, count, target)? {
        Cancel::Stay => CommandResult::effects_only(Effects::new()),
        Cancel::BufferStart => {
            CommandResult::new(Effects::new().set_cursor(Offset::new(0)), Offset::new(0))
        }
    })
}

/// Execute format operator keeping cursor position (gw).
///
/// Same as `execute` (gq) but the cursor stays on the text it was on
/// instead of moving to the first non-blank after the formatted region.
pub fn execute_keep_cursor(ctx: &OperatorContext<'_>) -> CommandResult {
    // A linewise Visual selection that ends on the empty last line keeps
    // the cursor there and formats its lines like the other selections.
    if from_empty_last_line(ctx) && ctx.origin != OperatorOrigin::Visual {
        return super::format_legacy::execute_keep_cursor(ctx);
    }
    // A Visual cursor on the end of a line that `$` did not put there is
    // where the engine's motion or text object stopped and Vim's moved on
    // to the next line, so the cursor cannot be kept on the text Vim's
    // was on. The operator works there as it did before it followed Vim,
    // with the width the Visual operators had then.
    if ctx.origin == OperatorOrigin::Visual
        && ctx.text.as_bytes().get(ctx.cursor.get()) == Some(&b'\n')
        && ctx.sticky_column != Some(crate::primitives::VirtualColumn::END_OF_LINE)
    {
        let before = ctx.clone().with_textwidth(VISUAL_WIDTH_BEFORE);
        return super::format_legacy::execute_keep_cursor(&before);
    }
    if ctx.is_empty() {
        return CommandResult::empty(ctx.cursor);
    }

    let formatted = format_range(ctx, Some(ctx.cursor.get()));
    let new_text = formatted.new_text(ctx.text);
    let cursor = formatted.kept_cursor.unwrap_or_else(|| ctx.cursor.get());
    // A cursor on the end of a line, which a Visual selection can leave,
    // stays there as it did before; Normal mode moves it back.
    // A Visual selection that ends on the empty last line leaves the
    // cursor on that line.
    let cursor = if from_empty_last_line(ctx) {
        Offset::new(new_text.len())
    } else if ctx.text.as_bytes().get(ctx.cursor.get()) == Some(&b'\n') {
        Offset::new(cursor.min(new_text.len()))
    } else {
        Offset::new(clamp_to_line(&new_text, cursor))
    };

    // Neovim's gw sets `[` = start, `]` = first non-blank of first line,
    // regardless of whether text changed (same as gq marks).
    let mark_start = Offset::new(formatted.range_start);
    let mark_end = Offset::new(begin_line(&new_text, formatted.first_line));

    if !formatted.changed() {
        return CommandResult::new(
            undo_step_without_change(&formatted)
                .set_mark(MarkName::CHANGE_START, mark_start, None)
                .set_mark(MarkName::CHANGE_END, mark_end, None)
                .set_cursor(cursor)
                .end_undo(),
            cursor,
        );
    }

    let effects = Effects::new()
        .begin_undo_force_entry()
        .replace(
            formatted.replaced_range(),
            CompactString::new(&formatted.text),
        )
        .set_mark(MarkName::CHANGE_START, mark_start, None)
        .set_mark(MarkName::CHANGE_END, mark_end, None)
        .set_cursor(cursor)
        .end_undo();
    CommandResult::new(effects, cursor)
}

/// Whether the command was given on the empty line after a final newline.
/// Undo cannot put the cursor back on that line, where Vim's `u` returns
/// it, so the operators work there as they did before they formatted whole
/// lines (see [`execute_as_before`]).
fn from_empty_last_line(ctx: &OperatorContext<'_>) -> bool {
    ctx.cursor.get() >= ctx.text.len() && ctx.text.ends_with('\n')
}

/// Vim saves the formatted lines for undo before it formats them, so `u`
/// after `gq` or `gw` puts the cursor back where the command was given,
/// also when formatting changed nothing. The lines are written back as
/// they are to make that undo step.
fn undo_step_without_change(
    formatted: &Formatted<'_>,
) -> Effects<crate::effects::undo_state::UndoOpen> {
    Effects::new().begin_undo_force_entry().replace(
        formatted.replaced_range(),
        CompactString::new(&formatted.text),
    )
}

// ── Range ────────────────────────────────────────────────────────────────────

/// The whole lines a format operator replaced and what replaced them.
#[derive(Debug)]
struct Formatted<'t> {
    /// Start of the operator's range, where the `[` mark goes.
    range_start: usize,
    /// Byte offset of the first formatted line.
    start: usize,
    /// Byte offset of the end of the last formatted line, before its newline.
    end: usize,
    /// The text the lines had.
    original: &'t str,
    /// The formatted lines, joined with newlines.
    text: String,
    /// Line index of the first formatted line in the buffer.
    first_line: usize,
    /// Index, among the formatted lines, of the line Vim's cursor ends on.
    last_line: usize,
    /// Vim moved the end of an exclusive motion back from column 0.
    end_adjusted: bool,
    /// For `gw`: where the cursor's text ended up, in the new buffer.
    kept_cursor: Option<usize>,
    /// A newline follows the last formatted line.
    followed_by_newline: bool,
}

impl Formatted<'_> {
    fn changed(&self) -> bool {
        self.text != self.original
    }

    const fn replaced_range(&self) -> Range {
        Range::from_raw(self.start, self.end)
    }

    fn new_text(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + self.text.len());
        out.push_str(text.get(..self.start).unwrap_or_default());
        out.push_str(&self.text);
        out.push_str(text.get(self.end..).unwrap_or_default());
        out
    }

    /// Offset in the new buffer right after the last formatted line with
    /// text, including its newline when one follows.
    fn change_end(&self) -> usize {
        let content = self.text.trim_end_matches('\n');
        let end = self.start + content.len();
        if content.len() < self.text.len() || self.followed_by_newline {
            end + 1
        } else {
            end
        }
    }
}

/// Format the lines `ctx.range` touches. `keep` is the `gw` cursor to
/// carry through the changes.
fn format_range<'t>(ctx: &OperatorContext<'t>, keep: Option<usize>) -> Formatted<'t> {
    let text = ctx.text;
    let mut range_start = ctx.range.start().get().min(text.len());
    // A linewise range that ends on the last line starts at the newline
    // before its first line, so that deleting it leaves no empty line. The
    // operator formats from the line after that newline.
    if ctx.motion_type.is_line_wise()
        && range_start > 0
        && text.as_bytes().get(range_start) == Some(&b'\n')
        && text.as_bytes().get(range_start - 1) != Some(&b'\n')
    {
        range_start = (line_end_for_offset(text, range_start) + 1).min(text.len());
    }
    let range_end = ctx.range.end().get().clamp(range_start, text.len());
    let last_included = if range_end > range_start {
        prev_char_boundary(text, range_end)
    } else {
        range_start
    };
    let start = line_start_for_offset(text, range_start);
    let end = line_end_for_offset(text, last_included);
    let original = text.get(start..end).unwrap_or_default();

    let prev = start
        .checked_sub(1)
        .and_then(|nl| text.get(line_start_for_offset(text, nl)..nl));
    let next = (end < text.len())
        .then(|| text.get(end + 1..line_end_for_offset(text, end + 1)))
        .flatten();

    // An exclusive motion that ends in column 0 of a later line stops at
    // the end of the line before (Vim's `end_adjusted`). The range then
    // ends where the motion did, or for a backward motion before it, when
    // the end was moved back to the last character of the line before, as
    // for `b` from an empty line. An inclusive motion and a linewise range
    // extended to whole lines end past the motion end instead. A forward
    // word motion that the engine stops at the end of the line stopped
    // there in Vim too.
    let motion_start = ctx.cursor.get().min(ctx.motion_target.get());
    let motion_end = ctx.cursor.get().max(ctx.motion_target.get());
    let backward = ctx.motion_target.get() < ctx.cursor.get();
    let end_adjusted = ctx.origin == OperatorOrigin::Motion
        && range_end > range_start
        && (range_end == motion_end || (backward && range_end < motion_end))
        && text.as_bytes().get(motion_end.wrapping_sub(1)) == Some(&b'\n')
        && crate::commands::helpers::line_of(text, motion_end)
            > crate::commands::helpers::line_of(text, motion_start);

    let defaults;
    let options = if let Some(options) = ctx.format_options {
        options
    } else {
        defaults = fallback_options(ctx);
        &defaults
    };
    let mut policy = FormatPolicy::from_options(options);
    if policy.textwidth == 0 {
        policy.textwidth = DEFAULT_FORMAT_WIDTH;
    }

    let mut lines: Vec<String> = original.split('\n').map(str::to_owned).collect();
    let mut mark = keep
        .filter(|&k| (start..=end).contains(&k))
        .map(|k| line_col(original, k - start));
    let last_line = format_lines(&mut lines, prev, next, &policy, mark.as_mut());
    // Vim leaves a line of only white space alone, as it ends a paragraph.
    // The operators emptied a range of such lines before they followed Vim,
    // as they wrapped the words of the range, and they still do: the engine
    // leaves the indent of an empty line that Vim removes when Insert mode
    // ends after autoindent, and Vim's buffer then has an empty line there.
    let range_text = text
        .get(ctx.range.start().get().min(text.len())..ctx.range.end().get().min(text.len()))
        .unwrap_or_default();
    let blank = |line: &String| line.bytes().all(|b| b == b' ' || b == b'\t');
    if lines.iter().all(blank)
        && (range_text.contains('\n')
            || unicode_width::UnicodeWidthStr::width(range_text) > policy.textwidth)
    {
        lines.iter_mut().for_each(String::clear);
    }
    let formatted = lines.join("\n");

    let delta = formatted.len().cast_signed() - original.len().cast_signed();
    let kept_cursor = keep.map(|k| match mark {
        Some((line, col)) => start + offset_of(&lines, line, col),
        None if k > end => k.saturating_add_signed(delta),
        None => k,
    });

    Formatted {
        range_start,
        start,
        end,
        original,
        text: formatted,
        first_line: crate::commands::helpers::line_of(text, start),
        last_line,
        end_adjusted,
        kept_cursor,
        followed_by_newline: end < text.len(),
    }
}

/// The options a context without format options formats with: the engine
/// defaults, with the context's `textwidth`, `tabstop` and `expandtab`.
fn fallback_options(ctx: &OperatorContext<'_>) -> VimOptions {
    let mut options = VimOptions::default();
    options.set_textwidth(ctx.textwidth);
    options.set_tabstop(ctx.tabstop);
    options.set_expandtab(ctx.expandtab);
    options
}

// ── format_lines() ───────────────────────────────────────────────────────────

/// A comment leader as `format_lines()` keeps it: its length, white space
/// included, and the part of `comments` that matched.
#[derive(Debug, Clone, Copy)]
struct Leader {
    len: usize,
    part: usize,
}

/// Vim's `fmt_check_par()` for one line: whether the line is outside any
/// paragraph, and its leader when `q` makes leaders count.
fn check_par(line: &str, comments: Option<&CommentSpec>) -> (bool, Option<Leader>) {
    let leader = comments.and_then(|spec| {
        spec.match_line(line)
            .filter(|m| m.ws_end > 0)
            .map(|m: LeaderMatch| Leader {
                len: m.ws_end,
                part: m.part,
            })
    });
    let len = leader.map_or(0, |l| l.len);
    let ends_comment = leader
        .zip(comments)
        .is_some_and(|(l, spec)| part_flags(spec, l).contains(CommentFlags::END));
    let rest = line.get(len..).unwrap_or_default();
    let not_par = rest.trim_start_matches([' ', '\t']).is_empty()
        || ends_comment
        || starts_paragraph_or_section(line);
    (not_par, leader)
}

fn part_flags(spec: &CommentSpec, leader: Leader) -> CommentFlags {
    spec.parts()
        .get(leader.part)
        .map_or(CommentFlags::empty(), CommentPart::flags)
}

/// Port of Vim's `format_lines()` over `lines`, the lines of the range.
///
/// `prev` and `next` are the lines right before and after the range, if
/// any. `mark` is a position (line index, byte column) carried through the
/// joins and breaks the way Vim adjusts marks. Returns the index of the
/// line the cursor ends on.
fn format_lines(
    lines: &mut Vec<String>,
    prev: Option<&str>,
    next: Option<&str>,
    policy: &FormatPolicy<'_>,
    mut mark: Option<&mut (usize, usize)>,
) -> usize {
    let flags = policy.flags;
    let comments = flags
        .contains(FormatFlags::FORMAT_COMMENTS)
        .then_some(policy.comments);
    let do_second_indent = flags.contains(FormatFlags::SECOND_LINE_INDENT);
    let do_trail_white = flags.contains(FormatFlags::WHITE_PARAGRAPH);
    // A joined line longer than this is formatted before the paragraph
    // ends, so a long paragraph is never one huge line.
    let max_len = policy.textwidth.saturating_mul(3);

    let (mut is_not_par, mut leader) = prev.map_or((true, None), |p| check_par(p, comments));
    let Some(first) = lines.first() else {
        return 0;
    };
    let (mut next_is_not_par, mut next_leader) = check_par(first, comments);
    let mut is_end_par = is_not_par || next_is_not_par;
    if !is_end_par && do_trail_white {
        is_end_par = !prev.is_some_and(ends_in_white);
    }

    let mut cur = 0;
    let mut started = false;
    let mut advance = true;
    let mut prev_is_end_par = false;
    let mut second_indent: Option<usize> = None;
    let mut com_list = false;
    let mut first_par_line = true;
    let mut need_set_indent = true;
    let mut force_format = false;

    let count = lines.len();
    for remaining in (1..=count).rev() {
        if advance {
            if started {
                cur += 1;
            }
            started = true;
            prev_is_end_par = is_end_par;
            is_not_par = next_is_not_par;
            leader = next_leader;
        }

        let last_of_buffer = remaining == 1 && next.is_none();
        if remaining == 1 {
            next_is_not_par = true;
            next_leader = None;
        } else {
            (next_is_not_par, next_leader) =
                check_par(lines.get(cur + 1).map_or("", String::as_str), comments);
        }
        advance = true;
        is_end_par = is_not_par || next_is_not_par;
        if !is_end_par && do_trail_white {
            is_end_par = !lines.get(cur).is_some_and(|l| ends_in_white(l));
        }

        // Lines outside a paragraph are left alone.
        if is_not_par {
            continue;
        }

        let leader_len = leader.map_or(0, |l| l.len);
        let next_leader_len = next_leader.map_or(0, |l| l.len);

        // For the first line of a paragraph, take the indent of the second.
        if first_par_line && do_second_indent && prev_is_end_par && !last_of_buffer {
            let second = if remaining == 1 {
                next
            } else {
                lines.get(cur + 1).map(String::as_str)
            };
            if let Some(second) = second.filter(|l| !l.is_empty()) {
                if leader_len == 0 && next_leader_len == 0 {
                    second_indent = Some(indent_width(second, policy.tabstop));
                } else {
                    second_indent = Some(next_leader_len);
                    com_list = true;
                }
            }
        }

        // A change of comment leader ends the paragraph. A line comment
        // after code followed by a line comment does not.
        let line = lines.get(cur).map_or("", String::as_str);
        let following = lines.get(cur + 1).map_or("", String::as_str);
        if last_of_buffer || !same_leader(line, leader, following, next_leader, comments) {
            // Vim reads the flags of the next line's leader. A line without
            // one gets the flags of the last 'comments' entry, where Vim's
            // get_leader_len() stopped scanning, so with c.vim's comments,
            // which end in "://", a line without a leader stays in the
            // paragraph after a line with a // comment.
            let continues_line_comment = comments.is_some_and(|spec| {
                let part =
                    next_leader.map_or_else(|| spec.parts().len().wrapping_sub(1), |l| l.part);
                is_plain_slash_comment(spec, part)
            }) && check_line_comment(line).is_some();
            if !continues_line_comment {
                is_end_par = true;
            }
        }

        // At the end of a paragraph, or when the line gets long, format it.
        if is_end_par || force_format {
            if need_set_indent {
                set_indent(lines, cur, policy, mark.as_deref_mut());
            }
            let call = OperatorCall {
                second_indent,
                com_list: comments.is_some() && com_list,
            };
            cur = break_line(lines, cur, policy, call, mark.as_deref_mut());
            second_indent = None;
            need_set_indent = is_end_par;
            if is_end_par {
                first_par_line = true;
            }
            force_format = false;
        }

        // Still in the same paragraph: join the next line, without its
        // leader.
        if !is_end_par {
            advance = false;
            let strip = if next_leader_len > 0 {
                next_leader_len
            } else if second_indent.is_some_and(|i| i > 0) {
                let following = lines.get(cur + 1).map_or("", String::as_str);
                following.len() - following.trim_start_matches([' ', '\t']).len()
            } else {
                0
            };
            join_next(lines, cur, strip, flags, mark.as_deref_mut());
            first_par_line = false;
            force_format = lines.get(cur).is_some_and(|l| l.len() > max_len);
        }
    }
    cur
}

/// Vim's `set_indent(get_indent(), SIN_CHANGED)` on the first line of a
/// paragraph: rebuild its indent from `tabstop` and `expandtab`.
fn set_indent(
    lines: &mut [String],
    cur: usize,
    policy: &FormatPolicy<'_>,
    mark: Option<&mut (usize, usize)>,
) {
    let Some(line) = lines.get_mut(cur) else {
        return;
    };
    let old_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let width = indent_width(line, policy.tabstop);
    let indent = build_indent_string(width, !policy.expandtab, policy.tabstop);
    if line.get(..old_len) == Some(indent.as_str()) {
        return;
    }
    line.replace_range(..old_len, &indent);
    if let Some((_, col)) = mark.filter(|m| m.0 == cur) {
        if *col >= old_len {
            *col = *col - old_len + indent.len();
        } else if *col >= indent.len() {
            *col = indent.len();
        }
    }
}

/// Break line `cur` at the margin. The new lines go in after it. Returns
/// the index of the last of them, where Vim's cursor ends.
fn break_line(
    lines: &mut Vec<String>,
    cur: usize,
    policy: &FormatPolicy<'_>,
    call: OperatorCall,
    mark: Option<&mut (usize, usize)>,
) -> usize {
    let Some(line) = lines.get(cur) else {
        return cur;
    };
    let (broken, edits) = format_line_for_operator(line, policy, call);
    if edits.is_empty() {
        return cur;
    }
    let added = broken.matches('\n').count();
    if let Some(mark) = mark {
        if mark.0 == cur {
            // Text that moves to a new line keeps its place in the text. A
            // position in the blanks removed at a break ends up at the end
            // of the line above, where Vim leaves it too once the cursor is
            // put back on the line.
            let (line, col) = line_col(&broken, map_offset(mark.1, &edits));
            *mark = (cur + line, col);
        } else if mark.0 > cur {
            mark.0 += added;
        }
    }
    lines.splice(cur..=cur, broken.split('\n').map(str::to_owned));
    cur + added
}

/// Join line `cur + 1` onto line `cur` the way Vim's `do_join()` does for
/// `format_lines()`, after deleting the first `strip` bytes (the leader)
/// of the joined line.
fn join_next(
    lines: &mut Vec<String>,
    cur: usize,
    strip: usize,
    flags: FormatFlags,
    mark: Option<&mut (usize, usize)>,
) {
    if cur + 1 >= lines.len() {
        return;
    }
    let next = lines.remove(cur + 1);
    let Some(line) = lines.get_mut(cur) else {
        return;
    };
    let after_leader = next.get(strip..).unwrap_or_default();
    let body = after_leader.trim_start_matches([' ', '\t']);
    let skipped = after_leader.len() - body.len();
    let spaces = join_spaces(line, body, flags);
    let joined_at = line.len();
    line.extend(std::iter::repeat_n(' ', spaces));
    line.push_str(body);

    if let Some(mark) = mark {
        if mark.0 == cur + 1 {
            // Vim's mark_col_adjust() for the deleted leader, then for the
            // join.
            let col = mark.1.saturating_sub(strip);
            let removed = skipped.cast_signed() - spaces.cast_signed();
            let col_amount = joined_at.cast_signed() - removed;
            let col = col.cast_signed();
            let new_col = if col_amount < 0 && col <= -col_amount {
                0
            } else if col < removed {
                col_amount + removed
            } else {
                col + col_amount
            };
            *mark = (cur, new_col.max(0).cast_unsigned());
        } else if mark.0 > cur + 1 {
            mark.0 -= 1;
        }
    }
}

/// How many spaces Vim's `do_join()` puts between `line` and the joined
/// text `body` (its leading blanks already removed).
fn join_spaces(line: &str, body: &str, flags: FormatFlags) -> usize {
    let Some(next) = body.chars().next() else {
        return 0;
    };
    let Some(end1) = line.chars().next_back() else {
        return 0;
    };
    if next == ')' || end1 == '\t' {
        return 0;
    }
    let wide = |c: char| u32::from(c) >= 0x100;
    if flags.contains(FormatFlags::MBYTE_JOIN) && (wide(next) || wide(end1)) {
        return 0;
    }
    if flags.contains(FormatFlags::MBYTE_JOIN_BETWEEN)
        && !((!wide(next) && !eats_space(end1)) || (!wide(end1) && !eats_space(next)))
    {
        return 0;
    }
    // No space after a line that already ends in one.
    usize::from(end1 != ' ')
}

/// Vim's `utf_eat_space()`: punctuation after which `B` adds no space.
fn eats_space(c: char) -> bool {
    matches!(u32::from(c),
        0x2000..=0x206F | 0x2E00..=0x2E7F | 0x3000..=0x303F | 0xFF01..=0xFF0F
        | 0xFF1A..=0xFF20 | 0xFF3B..=0xFF40 | 0xFF5B..=0xFF65)
}

/// Vim's `same_leader()`: whether line `line2` (the next one) continues the
/// comment of `line1`, so the two may be joined.
fn same_leader(
    line1: &str,
    leader1: Option<Leader>,
    line2: &str,
    leader2: Option<Leader>,
    comments: Option<&CommentSpec>,
) -> bool {
    let len2 = leader2.map_or(0, |l| l.len);
    let (Some(leader1), Some(spec)) = (leader1, comments) else {
        return len2 == 0;
    };
    let flags1 = part_flags(spec, leader1);
    // Vim takes the first of these flags in the order they are written; a
    // part has at most one of them in practice.
    if flags1.contains(CommentFlags::FIRST) {
        return len2 == 0;
    }
    if flags1.contains(CommentFlags::END) {
        return false;
    }
    if flags1.contains(CommentFlags::START) {
        if line1.len() <= leader1.len {
            return false;
        }
        return leader2.is_some_and(|l| part_flags(spec, l).contains(CommentFlags::MIDDLE));
    }

    // Compare the leaders, ignoring differences in white space.
    let b1 = line1.as_bytes();
    let b2 = line2.as_bytes();
    let mut idx1 = b1.iter().take_while(|&&b| b == b' ' || b == b'\t').count();
    let mut idx2 = 0;
    while idx2 < len2 {
        let c2 = b2.get(idx2).copied().unwrap_or(0);
        if c2 == b' ' || c2 == b'\t' {
            while b1.get(idx1).is_some_and(|&b| b == b' ' || b == b'\t') {
                idx1 += 1;
            }
        } else {
            let c1 = b1.get(idx1).copied().unwrap_or(0);
            idx1 += 1;
            if c1 != c2 {
                break;
            }
        }
        idx2 += 1;
    }
    idx2 == len2 && idx1 == leader1.len
}

/// Whether 'comments' part `part` is `://` exactly: Vim only lets a line
/// comment after code continue onto a line comment for that part.
fn is_plain_slash_comment(spec: &CommentSpec, part: usize) -> bool {
    spec.parts()
        .get(part)
        .is_some_and(|p| p.flags().is_empty() && p.offset() == 0 && p.string().starts_with("//"))
}

/// Vim's `check_linecomment()` without `lisp`: the byte column of a `//`
/// comment on `line` that is not inside a string.
fn check_line_comment(line: &str) -> Option<usize> {
    let b = line.as_bytes();
    let mut p = 0;
    while let Some(off) = b
        .get(p..)
        .and_then(|rest| rest.iter().position(|&c| c == b'/'))
    {
        p += off;
        let at = |i: usize| b.get(i).copied().unwrap_or(0);
        // A `*//*` is the end and start of a C comment, not a line comment.
        if at(p + 1) == b'/'
            && (p == 0 || at(p - 1) != b'*' || at(p + 2) != b'*')
            && !is_pos_in_string(b, p)
        {
            return Some(p);
        }
        p += 1;
    }
    None
}

/// Vim's `is_pos_in_string()`: whether `col` is inside a C string or
/// character literal on `line`.
fn is_pos_in_string(line: &[u8], col: usize) -> bool {
    let mut p = 0;
    while p < line.len() && p < col {
        p = skip_string(line, p) + 1;
    }
    p > col
}

/// Vim's `skip_string()`: from `p`, skip a C string, character literal or
/// raw string and return the position of its last byte, or `p` when none
/// starts there.
fn skip_string(line: &[u8], mut p: usize) -> usize {
    let at = |i: usize| line.get(i).copied().unwrap_or(0);
    loop {
        if at(p) == b'\'' {
            if at(p + 1) == 0 {
                break;
            }
            let mut i = 2;
            if at(p + 1) == b'\\' && at(p + 2) != 0 {
                i += 1;
                while at(p + i - 1).is_ascii_digit() {
                    i += 1;
                }
            }
            if at(p + i - 1) != 0 && at(p + i) == b'\'' {
                p += i + 1;
                continue;
            }
        } else if at(p) == b'"' {
            p += 1;
            while at(p) != 0 {
                if at(p) == b'\\' && at(p + 1) != 0 {
                    p += 1;
                } else if at(p) == b'"' {
                    break;
                }
                p += 1;
            }
            if at(p) == b'"' {
                p += 1;
                continue;
            }
        } else if at(p) == b'R' && at(p + 1) == b'"' {
            let delim_start = p + 2;
            if let Some(paren) = line
                .get(delim_start..)
                .and_then(|rest| rest.iter().position(|&c| c == b'('))
            {
                let delim = line
                    .get(delim_start..delim_start + paren)
                    .unwrap_or_default();
                p += 3;
                while at(p) != 0 {
                    if at(p) == b')'
                        && line.get(p + 1..p + 1 + delim.len()) == Some(delim)
                        && at(p + delim.len() + 1) == b'"'
                    {
                        p += delim.len() + 1;
                        break;
                    }
                    p += 1;
                }
                if at(p) == b'"' {
                    p += 1;
                    continue;
                }
            }
        }
        break;
    }
    if at(p) == 0 && p > 0 {
        p -= 1;
    }
    p
}

/// Vim's `startPS(lnum, NUL, false)`: an empty line, a form feed, or an
/// nroff paragraph or section macro.
pub(super) fn starts_paragraph_or_section(line: &str) -> bool {
    match line.as_bytes() {
        [] | [b'\x0c', ..] => true,
        [b'.', rest @ ..] => in_macro(SECTION_MACROS, rest) || in_macro(PARAGRAPH_MACROS, rest),
        _ => false,
    }
}

/// Vim's `inmacro()`: whether `s` starts with one of the two-letter macro
/// names in `macros`. A space in a name matches a space or the end of the
/// line.
fn in_macro(macros: &str, s: &[u8]) -> bool {
    let s0 = s.first().copied().unwrap_or(0);
    let s1 = s.get(1).copied().unwrap_or(0);
    let m = macros.as_bytes();
    let mut i = 0;
    while let Some(&m0) = m.get(i) {
        let m1 = m.get(i + 1).copied().unwrap_or(0);
        let first = m0 == s0 || (m0 == b' ' && (s0 == 0 || s0 == b' '));
        let second = m1 == s1 || ((m1 == 0 || m1 == b' ') && (s0 == 0 || s1 == 0 || s1 == b' '));
        if first && second {
            return true;
        }
        if m1 == 0 {
            break;
        }
        i += 2;
    }
    false
}

fn ends_in_white(line: &str) -> bool {
    line.ends_with([' ', '\t'])
}

// ── Positions ────────────────────────────────────────────────────────────────

/// Width of the leading white space of `line` in display columns.
fn indent_width(line: &str, tabstop: usize) -> usize {
    let tabstop = tabstop.max(1);
    line.bytes()
        .take_while(|&b| b == b' ' || b == b'\t')
        .fold(0, |col, b| {
            if b == b'\t' {
                col + tabstop - col % tabstop
            } else {
                col + 1
            }
        })
}

fn line_count(text: &str) -> usize {
    text.matches('\n').count() + 1
}

/// Vim's `beginline(BL_WHITE | BL_FIX)` on line `line`: the first
/// character that is not a blank, or the last blank of a blank line.
fn begin_line(text: &str, line: usize) -> usize {
    let start = crate::commands::helpers::line_start(text, line).unwrap_or(text.len());
    let end = line_end_for_offset(text, start);
    let content = text.get(start..end).unwrap_or_default();
    let blanks = content
        .bytes()
        .take_while(|&b| b == b' ' || b == b'\t')
        .count();
    if blanks == content.len() {
        start + blanks.saturating_sub(1)
    } else {
        start + blanks
    }
}

/// Keep a Normal-mode cursor on its line: at most on the last character.
fn clamp_to_line(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    let start = line_start_for_offset(text, offset);
    let end = line_end_for_offset(text, offset);
    if offset < end || end == start {
        return offset;
    }
    text.get(start..end)
        .and_then(|l| l.grapheme_indices(true).next_back())
        .map_or(start, |(i, _)| start + i)
}

/// Line index and byte column of `offset` in `text`.
fn line_col(text: &str, offset: usize) -> (usize, usize) {
    let before = text.get(..offset).unwrap_or(text);
    let line = before.matches('\n').count();
    let col = before
        .rfind('\n')
        .map_or(before.len(), |nl| before.len() - nl - 1);
    (line, col)
}

/// Byte offset of (`line`, `col`) in `lines` joined with newlines, with the
/// column clamped to the line and to a character boundary.
fn offset_of(lines: &[String], line: usize, col: usize) -> usize {
    let before: usize = lines.iter().take(line).map(|l| l.len() + 1).sum();
    before + lines.get(line).map_or(0, |l| l.floor_char_boundary(col))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::DEFAULT_COMMENTS;

    fn format(lines: &[&str], tw: usize, fo: &str, comments: &str, ai: bool) -> Vec<String> {
        let spec = CommentSpec::parse(comments).unwrap();
        let policy = FormatPolicy {
            textwidth: tw,
            flags: FormatFlags::parse(fo).unwrap(),
            comments: &spec,
            tabstop: 8,
            autoindent: ai,
            expandtab: false,
        };
        let mut lines: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
        format_lines(&mut lines, None, None, &policy, None);
        lines
    }

    #[test]
    fn hash_block_reflows_inside_the_comment() {
        assert_eq!(
            format(
                &["# aa bb cc dd ee ff", "# gg", "# hh ii jj kk ll mm nn oo"],
                20,
                "tcq",
                DEFAULT_COMMENTS,
                false
            ),
            ["# aa bb cc dd ee ff", "# gg hh ii jj kk ll", "# mm nn oo"]
        );
    }

    #[test]
    fn without_q_leaders_are_text() {
        assert_eq!(
            format(&["# aa bb", "# cc dd"], 20, "tc", DEFAULT_COMMENTS, false),
            ["# aa bb # cc dd"]
        );
    }

    #[test]
    fn leader_only_and_blank_lines_end_paragraphs() {
        assert_eq!(
            format(
                &["# aa", "#", "# bb", "   ", "cc", "dd"],
                20,
                "tcq",
                DEFAULT_COMMENTS,
                false
            ),
            ["# aa", "#", "# bb", "   ", "cc dd"]
        );
    }

    #[test]
    fn leader_change_ends_a_paragraph() {
        assert_eq!(
            format(&["# aa", "## bb", "## cc"], 20, "tcq", "b:##,b:#", false),
            ["# aa", "## bb cc"]
        );
    }

    #[test]
    fn long_paragraph_is_formatted_as_it_is_joined() {
        let words: Vec<String> = (0..200).map(|i| format!("w{i:03}")).collect();
        let lines: Vec<&str> = words.iter().map(String::as_str).collect();
        let out = format(&lines, 20, "tcq", DEFAULT_COMMENTS, false);
        assert!(out.iter().all(|l| l.len() <= 20), "{out:?}");
        assert_eq!(out.join(" ").split(' ').count(), 200);
    }

    #[test]
    fn join_spacing_follows_do_join() {
        let fo = FormatFlags::empty();
        assert_eq!(join_spaces("aa", "bb", fo), 1);
        assert_eq!(join_spaces("aa ", "bb", fo), 0);
        assert_eq!(join_spaces("aa\t", "bb", fo), 0);
        assert_eq!(join_spaces("aa", ")", fo), 0);
        assert_eq!(join_spaces("", "bb", fo), 0);
        assert_eq!(join_spaces("aa", "", fo), 0);
        assert_eq!(join_spaces("日本", "語", FormatFlags::MBYTE_JOIN), 0);
        assert_eq!(
            join_spaces("日本", "語", FormatFlags::MBYTE_JOIN_BETWEEN),
            0
        );
        assert_eq!(
            join_spaces("日本", "ab", FormatFlags::MBYTE_JOIN_BETWEEN),
            1
        );
        assert_eq!(
            join_spaces("日本。", "ab", FormatFlags::MBYTE_JOIN_BETWEEN),
            0
        );
    }

    #[test]
    fn nroff_macros_and_form_feeds_are_not_paragraph_lines() {
        assert!(starts_paragraph_or_section(".PP"));
        assert!(starts_paragraph_or_section(".P"));
        assert!(starts_paragraph_or_section(".ip_address"));
        assert!(starts_paragraph_or_section(".SH NAME"));
        assert!(starts_paragraph_or_section("\x0cpage"));
        assert!(!starts_paragraph_or_section(".foo()"));
        assert!(!starts_paragraph_or_section("text"));
    }

    #[test]
    fn line_comment_outside_strings_only() {
        assert_eq!(check_line_comment("x = 1; // c"), Some(7));
        assert_eq!(check_line_comment("s = \"//\" + t"), None);
        assert_eq!(check_line_comment("s = \"//\" // c"), Some(9));
        assert_eq!(check_line_comment("a */ /* b"), None);
        assert_eq!(check_line_comment("c = '/'; // d"), Some(9));
        assert_eq!(check_line_comment("no comment"), None);
    }

    #[test]
    fn leaders_compare_without_white_space() {
        let spec = CommentSpec::parse(DEFAULT_COMMENTS).unwrap();
        let leader = |line: &str| check_par(line, Some(&spec)).1;
        let same = |a: &str, b: &str| same_leader(a, leader(a), b, leader(b), Some(&spec));
        assert!(same("# aa", "# bb"));
        assert!(same("# aa", "  # bb"));
        assert!(!same("# aa", "// bb"));
        assert!(!same("# aa", "bb"));
        assert!(same("aa", "bb"));
        // `fb:-`: a list item continues only on lines without a leader.
        assert!(!same("- aa", "- bb"));
        // A three-piece start continues on a middle line.
        assert!(same("/* aa", " * bb"));
        assert!(!same("/*", " * bb"));
    }

    #[test]
    fn kept_cursor_lands_on_a_character_boundary() {
        let lines = ["/* 日本".to_owned(), " * bb".to_owned()];
        assert_eq!(offset_of(&lines, 0, 5), 3);
        assert_eq!(offset_of(&lines, 0, 99), 9);
        assert_eq!(offset_of(&lines, 1, 3), 13);
    }

    #[test]
    fn test_begin_line() {
        assert_eq!(begin_line("  hello", 0), 2);
        assert_eq!(begin_line("hello", 0), 0);
        assert_eq!(begin_line("\thello", 0), 1);
        assert_eq!(begin_line("a\n   \nb", 1), 4);
        assert_eq!(begin_line("a\n\nb", 1), 2);
    }
}
