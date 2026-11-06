//! Where each command of an Ex line ends.
//!
//! Vim decides it per command: `do_one_cmd()` reads the modifiers, the
//! range and the command name, then the command's `EX_*` flags in
//! `ex_cmds.h` say how its argument ends. A command with `EX_TRLBAR` ends
//! at `|` or a `"` comment (`separate_nextcmd()`), an expression ends after
//! its last operand (`eval0_retarg()`), `:s` after its flags
//! (`ex_substitute()`), and the rest take the rest of the line.

use std::borrow::Cow;

use super::command_meta::{find_command, is_pattern_delimiter, FoundCommand, OwnKind};
use super::ex_parser::parse_modifier_prefixes;
use super::script::heredoc_start;
use super::vim_commands::{ex, VimCommand, MODIFIERS};
use crate::grammar::types::ModifierFlags;

/// The commands of `line`, each with its text as Vim's rules leave it.
pub(crate) fn split(line: &str) -> Vec<Cow<'_, str>> {
    let mut pieces = Vec::new();
    let mut start = Some(0);
    while let Some(at) = start {
        let piece = piece_at(line, at);
        if let Some(text) = piece.text(line) {
            pieces.push(text);
        }
        start = piece.next;
    }
    pieces
}

/// How the end of a command's text is trimmed.
#[derive(Debug, Clone, Copy)]
enum Trim {
    /// Kept as it is.
    None,
    /// Trailing spaces and tabs go.
    Blanks,
    /// The argument starts at `arg`; with `del`, its trailing blanks go as
    /// Vim's `del_trailing_spaces()` removes them.
    Arg { arg: usize, del: bool },
}

/// One command of a line.
#[derive(Debug)]
struct Piece {
    start: usize,
    end: usize,
    /// Single bytes left out of the text: a backslash before `|` or `"`, or
    /// a Ctrl-V.
    dropped: Vec<usize>,
    trim: Trim,
    /// Where the next command starts.
    next: Option<usize>,
}

impl Piece {
    /// No command, then the next one at `next`.
    const fn nothing(at: usize, next: Option<usize>) -> Self {
        Self {
            start: at,
            end: at,
            dropped: Vec::new(),
            trim: Trim::None,
            next,
        }
    }

    /// The text from `start` to the end of the line.
    const fn rest_of_line(line: &str, start: usize) -> Self {
        Self {
            start,
            end: line.len(),
            dropped: Vec::new(),
            trim: Trim::None,
            next: None,
        }
    }

    /// The text from `start` to `end`, without its trailing blanks.
    const fn ending(line: &str, start: usize, end: ArgEnd) -> Self {
        let (end, next) = match end {
            ArgEnd::Bar(at) => (at, Some(at + 1)),
            ArgEnd::Comment(at) => (at, None),
            ArgEnd::End => (line.len(), None),
        };
        Self {
            start,
            end,
            dropped: Vec::new(),
            trim: Trim::Blanks,
            next,
        }
    }

    fn text<'a>(&self, line: &'a str) -> Option<Cow<'a, str>> {
        let raw = line
            .get(self.start..self.end)
            .filter(|raw| !raw.is_empty())?;
        let mut text = if self.dropped.is_empty() {
            Cow::Borrowed(raw)
        } else {
            let mut owned = String::with_capacity(raw.len());
            let mut from = self.start;
            for &at in &self.dropped {
                owned.push_str(line.get(from..at)?);
                from = at + 1;
            }
            owned.push_str(line.get(from..self.end)?);
            Cow::Owned(owned)
        };
        let keep = match self.trim {
            Trim::None => text.len(),
            Trim::Blanks => trimmed_len(&text, 0),
            Trim::Arg { arg, .. } if text.len() <= arg - self.start => trimmed_len(&text, 0),
            Trim::Arg { arg, del: true } => del_trailing_spaces(&text, arg - self.start),
            Trim::Arg { del: false, .. } => text.len(),
        };
        match &mut text {
            Cow::Borrowed(s) => *s = s.get(..keep)?,
            Cow::Owned(s) => s.truncate(keep),
        }
        (!text.is_empty()).then_some(text)
    }
}

/// The length of `text` without its trailing spaces and tabs, keeping the
/// first `keep` bytes.
fn trimmed_len(text: &str, keep: usize) -> usize {
    let bytes = text.as_bytes();
    let mut len = bytes.len();
    while len > keep && matches!(bytes.get(len - 1), Some(b' ' | b'\t')) {
        len -= 1;
    }
    len
}

/// Vim's `del_trailing_spaces()` on the argument at `arg`: a blank after a
/// backslash or a Ctrl-V stays, and so does the argument's first byte.
fn del_trailing_spaces(text: &str, arg: usize) -> usize {
    let bytes = text.as_bytes();
    let mut len = bytes.len();
    while len > arg + 1
        && matches!(bytes.get(len - 1), Some(b' ' | b'\t'))
        && !matches!(bytes.get(len - 2), Some(b'\\' | 0x16))
    {
        len -= 1;
    }
    len
}

const CTRL_V: u8 = 0x16;

/// The length of the character whose first byte is at `at`.
fn char_len(bytes: &[u8], at: usize) -> usize {
    match bytes.get(at) {
        Some(0xf0..) => 4,
        Some(0xe0..) => 3,
        Some(0xc0..) => 2,
        _ => 1,
    }
}

fn skip_blanks(bytes: &[u8], mut at: usize) -> usize {
    while matches!(bytes.get(at), Some(b' ' | b'\t')) {
        at += 1;
    }
    at
}

/// The row of Vim's table for the command of `piece`, found past its
/// colons, modifiers and range as `do_one_cmd()` finds it, with the bytes
/// where its name starts and ends. `None` for no command, a name Vim does
/// not know and the engine's own commands.
pub(super) fn vim_command_at(piece: &str) -> Option<(&'static VimCommand, usize, usize)> {
    command_head(piece).map(|head| (head.row, head.name, head.name_end))
}

/// What comes before the argument of a Vim command.
pub(super) struct Head {
    pub(super) row: &'static VimCommand,
    /// The engine's modifiers among those before the command.
    pub(super) modifiers: ModifierFlags,
    /// Whether a range comes before the name.
    pub(super) range: bool,
    pub(super) name: usize,
    pub(super) name_end: usize,
}

/// The command of `piece` as [`vim_command_at`] finds it, and what comes
/// before it.
pub(super) fn command_head(piece: &str) -> Option<Head> {
    let bytes = piece.as_bytes();
    let mut at = skip_blanks(bytes, 0);
    let mut modifiers = ModifierFlags::empty();
    loop {
        let (flags, rest) = parse_modifier_prefixes(piece.get(at..)?);
        modifiers |= flags;
        let cmd = piece.len() - rest.len();
        let name = skip_range(bytes, cmd);
        let (FoundCommand::Vim(row), len) = find_command(piece.get(name..)?)? else {
            return None;
        };
        match modifier_end(piece, cmd, name, len, row) {
            Some(after) => at = after,
            None => {
                return Some(Head {
                    row,
                    modifiers,
                    range: bytes
                        .get(cmd..name)
                        .is_some_and(|range| range.iter().any(|b| !b" \t:".contains(b))),
                    name,
                    name_end: name + len,
                })
            }
        }
    }
}

/// The command that starts at `at`, the first byte after a `|`.
fn piece_at(line: &str, at: usize) -> Piece {
    let bytes = line.as_bytes();
    // `do_one_cmd()`: "#!" is a comment only as the first two bytes.
    if bytes.get(at..at + 2) == Some(b"#!") {
        return Piece::nothing(at, None);
    }
    command_piece(line, skip_blanks(bytes, at))
}

/// The command that starts at `start`: colons, modifiers, a range, the
/// command name and its argument.
fn command_piece(line: &str, start: usize) -> Piece {
    let bytes = line.as_bytes();
    let mut at = start;
    loop {
        let (_, rest) = parse_modifier_prefixes(line.get(at..).unwrap_or_default());
        let cmd = line.len() - rest.len();
        match bytes.get(cmd) {
            None => return Piece::nothing(cmd, None),
            // `parse_command_modifiers()`: a comment, which ends at a newline.
            Some(b'"') => {
                let next = line[cmd..].find('\n').map(|nl| cmd + nl + 1);
                return Piece::nothing(cmd, next);
            }
            _ => {}
        }
        let name = skip_range(bytes, cmd);
        if matches!(bytes.get(name), None | Some(b'|' | b'\n' | b'"')) {
            let next = match bytes.get(name) {
                Some(b'|' | b'\n') => Some(name + 1),
                _ => None,
            };
            if name == cmd {
                return Piece::nothing(cmd, next);
            }
            // A range without a command.
            let mut end = name;
            while end > cmd && matches!(bytes.get(end - 1), Some(b' ' | b'\t' | b':')) {
                end -= 1;
            }
            return Piece {
                start,
                end,
                dropped: Vec::new(),
                trim: Trim::None,
                next,
            };
        }
        return match find_command(&line[name..]) {
            // Vim's `E492` path never looks for `|`.
            None => Piece::rest_of_line(line, start),
            Some((FoundCommand::Structural, len)) => pattern_then_command(line, start, name + len),
            Some((FoundCommand::Own(own), len)) => {
                let mut arg = name + len;
                while bytes.get(arg) == Some(&b'!') {
                    arg += 1;
                }
                let arg = skip_blanks(bytes, arg);
                match own.kind {
                    OwnKind::Bar => separate_nextcmd(line, start, arg, ex::TRLBAR, ""),
                    OwnKind::Plain => first_bar(line, start, arg),
                    OwnKind::Pattern => first_bar(line, start, skip_pattern_arg(line, arg)),
                }
            }
            Some((FoundCommand::Vim(row), len)) => {
                if let Some(after) = modifier_end(line, cmd, name, len, row) {
                    at = after;
                    continue;
                }
                vim_command_piece(line, start, name, len, row)
            }
        };
    }
}

/// Past a modifier the engine does not know, such as `:verbose`, and its
/// argument, as `parse_command_modifiers()` skips it, so that the command
/// after it decides where the piece ends.
fn modifier_end(
    line: &str,
    cmd: usize,
    name: usize,
    len: usize,
    row: &VimCommand,
) -> Option<usize> {
    let bytes = line.as_bytes();
    let &(_, _, takes_count) = MODIFIERS.iter().find(|(full, _, _)| *full == row.name)?;
    let mut p = name + len;
    if (name != cmd && !takes_count) || bytes.get(p) == Some(&b'_') {
        return None;
    }
    match row.name {
        "silent" if bytes.get(p) == Some(&b'!') => p += 1,
        // `:hide` alone, or before `|` or a comment, is the command.
        "hide" if ends_excmd2(bytes.get(skip_blanks(bytes, p))) => return None,
        "filter" => {
            p = skip_blanks(bytes, p);
            if bytes.get(p) == Some(&b'!') {
                p = skip_blanks(bytes, p + 1);
            }
            if ends_excmd2(bytes.get(p)) {
                return None;
            }
            p = skip_vimgrep_pat(line, p)?;
        }
        _ => {}
    }
    Some(p)
}

/// Vim's `ends_excmd2()` for the byte `byte`: the end of the line, a `|`, a
/// newline or a `"` comment.
const fn ends_excmd2(byte: Option<&u8>) -> bool {
    matches!(byte, None | Some(b'|' | b'\n' | b'"'))
}

/// Vim's `skip_range()`: the range at `at` and the colons, blanks and
/// Visual `*` after it.
pub(super) fn skip_range(bytes: &[u8], at: usize) -> usize {
    let mut p = at;
    while let Some(&byte) = bytes.get(p) {
        if !b" \t0123456789.$%'/?-+,;\\".contains(&byte) {
            break;
        }
        match byte {
            b'\\' => {
                if matches!(bytes.get(p + 1), Some(b'?' | b'/' | b'&')) {
                    p += 1;
                } else {
                    break;
                }
            }
            b'\'' => {
                // A quote is a mark only at the start or after a separator.
                let before = bytes
                    .get(at..p)
                    .and_then(|run| run.iter().rev().find(|b| !matches!(b, b' ' | b'\t')));
                if before.is_some_and(|b| *b != b',' && *b != b';') {
                    break;
                }
                p += 1;
            }
            b'/' | b'?' => {
                p += 1;
                while let Some(&b) = bytes.get(p) {
                    if b == byte {
                        break;
                    }
                    if b == b'\\' && p + 1 < bytes.len() {
                        p += 1;
                    }
                    p += 1;
                }
            }
            _ => {}
        }
        if p < bytes.len() {
            p += char_len(bytes, p);
        }
    }
    while bytes.get(p) == Some(&b':') {
        p = skip_blanks(bytes, p + 1);
    }
    if bytes.get(p) == Some(&b'*') {
        p = skip_blanks(bytes, p + 1);
    }
    p
}

/// A command of Vim's table, whose name of `len` bytes starts at `name`.
fn vim_command_piece(line: &str, start: usize, name: usize, len: usize, row: &VimCommand) -> Piece {
    let bytes = line.as_bytes();
    let mut p = name + len;
    // `:s` followed by a delimiter, as the parser reads it.
    if len == 1 && row.name == "substitute" && is_pattern_delimiter(bytes.get(p).copied()) {
        let delim = line[p..].chars().next();
        return substitute_piece(line, start, p, delim);
    }
    // `do_one_cmd()`: a `!` after the name, except for the `:s` commands.
    let bang =
        bytes.get(p) == Some(&b'!') && !matches!(row.name, "substitute" | "smagic" | "snomagic");
    if bang {
        p += 1;
    }
    let arg = if row.name == "!" {
        p
    } else {
        skip_blanks(bytes, p)
    };
    let mut after_opts = arg;
    if row.flags & ex::ARGOPT != 0 {
        while bytes.get(after_opts..after_opts + 2) == Some(b"++") {
            while !matches!(bytes.get(after_opts), None | Some(b' ' | b'\t')) {
                after_opts += 1;
            }
            after_opts = skip_blanks(bytes, after_opts);
        }
    }
    let filter = match row.name {
        "read" => bang || bytes.get(after_opts) == Some(&b'!'),
        "write" => bytes.get(after_opts) == Some(&b'!'),
        _ => false,
    };
    let mut arg = after_opts;
    // `getargcmd()`: a `+cmd` argument ends at a blank.
    if row.flags & ex::CMDARG != 0 && !filter && bytes.get(arg) == Some(&b'+') {
        arg += 1;
        while let Some(&b) = bytes.get(arg) {
            if matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
                break;
            }
            if b == b'\\' && arg + 1 < bytes.len() {
                arg += 1;
            }
            arg += char_len(bytes, arg);
        }
        arg = skip_blanks(bytes, arg);
    }

    if row.flags & ex::TRLBAR != 0 && !filter {
        return separate_nextcmd(line, start, arg, row.flags, row.name);
    }
    if filter || matches!(row.name, "!" | "terminal" | "global" | "vglobal") {
        return shell_piece(line, start, arg);
    }
    // `ex_let()` gives a heredoc the rest of the line.
    if matches!(row.name, "let" | "const") && heredoc_start(&line[arg..]).is_some() {
        return Piece::rest_of_line(line, start);
    }
    if row.flags & ex::EXPR_ARG != 0
        || matches!(
            row.name,
            "while" | "return" | "throw" | "unlet" | "lockvar" | "unlockvar" | "final" | "import"
        )
    {
        let echo = matches!(
            row.name,
            "echo" | "echon" | "echomsg" | "echoerr" | "echoconsole" | "echowindow" | "execute"
        );
        return match expression_end(line, arg, echo) {
            ArgEnd::End => Piece::rest_of_line(line, start),
            end => Piece::ending(line, start, end),
        };
    }
    if matches!(row.name, "substitute" | "smagic" | "snomagic" | "&" | "~") {
        // `ex_substitute()`: only an `s` command takes a new pattern.
        let delim = match line[arg..].chars().next() {
            Some(c) if row.name.starts_with('s') && !"0123456789cegriIp|\"".contains(c) => {
                if c.is_ascii_alphabetic() {
                    return Piece::rest_of_line(line, start);
                }
                Some(c)
            }
            _ => None,
        };
        return substitute_piece(line, start, arg, delim);
    }
    own_argument_end(line, arg, row.name).map_or_else(
        || Piece::rest_of_line(line, start),
        |end| Piece::ending(line, start, end),
    )
}

/// Where the argument at `arg` ends for the Vim commands without
/// `EX_TRLBAR` that read their own argument and then look for a `|`, as
/// their functions in Vim do; `None` for the others, which take the rest of
/// the line.
fn own_argument_end(line: &str, arg: usize, name: &str) -> Option<ArgEnd> {
    let bytes = line.as_bytes();
    Some(match name {
        // `ex_catch()`: the first `|` after the pattern.
        "catch" if ends_excmd2(bytes.get(arg)) => find_nextcmd(bytes, arg),
        "catch" => match pattern_at(line, arg) {
            Some(delim_at) => find_nextcmd(bytes, delim_at),
            None => ArgEnd::End,
        },
        "match" => match_end(line, arg),
        "sort" => sort_end(line, arg),
        "help" => help_end(bytes, arg),
        "wincmd" => wincmd_end(bytes, arg),
        "dsearch" | "dlist" | "djump" | "dsplit" | "isearch" | "ilist" | "ijump" | "isplit"
        | "psearch" => findpat_end(line, arg),
        "delfunction" => after_argument(bytes, function_name_end(bytes, arg)),
        "function" => return function_list_end(line, arg),
        "syntax" => syntax_end(line, arg),
        _ => return None,
    })
}

/// Vim's `find_nextcmd()`: the first `|` or newline from `from`.
fn find_nextcmd(bytes: &[u8], from: usize) -> ArgEnd {
    bytes
        .get(from..)
        .and_then(|rest| rest.iter().position(|b| matches!(b, b'|' | b'\n')))
        .map_or(ArgEnd::End, |at| ArgEnd::Bar(from + at))
}

/// What a command finds after its argument at `at`, as `ends_excmd2()` and
/// `set_nextcmd()` see it: a `|` or newline, a `"` comment, or anything
/// else, which is an error that takes the rest of the line.
fn after_argument(bytes: &[u8], at: usize) -> ArgEnd {
    let p = skip_blanks(bytes, at);
    match bytes.get(p) {
        Some(b'|' | b'\n') => ArgEnd::Bar(p),
        Some(b'"') => ArgEnd::Comment(p),
        _ => ArgEnd::End,
    }
}

/// The closing delimiter of a pattern that opens at `at` with its first
/// character, as `skip_regexp()` finds it; `None` when the line ends first.
fn pattern_at(line: &str, at: usize) -> Option<usize> {
    let delim = line.get(at..)?.chars().next()?;
    let (after, closed) = skip_past(line, at + delim.len_utf8(), delim);
    closed.then(|| after - delim.len_utf8())
}

/// `ex_match()`: `none` or a group and a pattern, then the first `|`.
fn match_end(line: &str, arg: usize) -> ArgEnd {
    let bytes = line.as_bytes();
    if ends_excmd2(bytes.get(arg)) {
        return find_nextcmd(bytes, arg);
    }
    let none = bytes
        .get(arg..arg + 4)
        .is_some_and(|word| word.eq_ignore_ascii_case(b"none"));
    if none && (ends_excmd2(bytes.get(arg + 4)) || matches!(bytes.get(arg + 4), Some(b' ' | b'\t')))
    {
        return find_nextcmd(bytes, arg + 4);
    }
    let mut group_end = arg;
    while !matches!(bytes.get(group_end), None | Some(b' ' | b'\t')) {
        group_end += 1;
    }
    match pattern_at(line, skip_blanks(bytes, group_end)) {
        Some(end) if ends_excmd2(bytes.get(skip_blanks(bytes, end + 1))) => {
            find_nextcmd(bytes, end)
        }
        _ => ArgEnd::End,
    }
}

/// `ex_sort()`: its letters, a pattern and a `"` comment, then a `|`.
/// Deviation: after an argument Vim rejects with E474, the first `|`.
fn sort_end(line: &str, arg: usize) -> ArgEnd {
    let bytes = line.as_bytes();
    let mut p = arg;
    let mut pattern = false;
    while let Some(&byte) = bytes.get(p) {
        match byte {
            b' ' | b'\t' => p += 1,
            b'"' => return ArgEnd::Comment(p),
            b'|' | b'\n' => return ArgEnd::Bar(p),
            b if b.is_ascii_alphabetic() => p += 1,
            _ if !pattern => {
                let Some(end) = pattern_at(line, p) else {
                    return ArgEnd::End;
                };
                p = end + char_len(bytes, end);
                pattern = true;
            }
            _ => return find_nextcmd(bytes, p),
        }
    }
    ArgEnd::End
}

/// `ex_help()`: a newline or carriage return, or a `|` followed by a
/// character that is not `|`.
fn help_end(bytes: &[u8], arg: usize) -> ArgEnd {
    let mut p = arg;
    while let Some(&byte) = bytes.get(p) {
        let bar = byte == b'|' && !matches!(bytes.get(p + 1), None | Some(b'|'));
        if bar || byte == b'\n' || byte == b'\r' {
            return ArgEnd::Bar(p);
        }
        p += 1;
    }
    ArgEnd::End
}

/// `ex_wincmd()`: one character, two after `g` or Ctrl-G, so `:wincmd |`
/// keeps its `|`.
fn wincmd_end(bytes: &[u8], arg: usize) -> ArgEnd {
    let mut p = arg;
    if matches!(bytes.get(p), Some(b'g' | 0x07)) {
        p += 1;
    }
    if p >= bytes.len() || char_len(bytes, p) > 1 {
        return ArgEnd::End;
    }
    after_argument(bytes, p + 1)
}

/// `ex_findpat()`: a count, then a `/pattern/` and what follows it; any
/// other argument is the rest of the line.
fn findpat_end(line: &str, arg: usize) -> ArgEnd {
    let bytes = line.as_bytes();
    let mut p = arg;
    while bytes.get(p).is_some_and(u8::is_ascii_digit) {
        p += 1;
    }
    p = skip_blanks(bytes, p);
    if bytes.get(p) != Some(&b'/') {
        return ArgEnd::End;
    }
    pattern_at(line, p).map_or(ArgEnd::End, |end| after_argument(bytes, end + 1))
}

/// The end of a function name at `at`. Deviation: Vim reads the name as an
/// expression (`trans_function_name_ext()`); here it runs to a blank, `(`,
/// `|`, `"` or newline.
fn function_name_end(bytes: &[u8], at: usize) -> usize {
    let mut p = at;
    while !matches!(
        bytes.get(p),
        None | Some(b' ' | b'\t' | b'(' | b'|' | b'"' | b'\n')
    ) {
        p += 1;
    }
    p
}

/// `ex_function()` when it lists functions: no argument, a `/pattern`, or a
/// name with no `(` anywhere after it. `None` for a definition, which
/// takes the rest of the line.
fn function_list_end(line: &str, arg: usize) -> Option<ArgEnd> {
    let bytes = line.as_bytes();
    if ends_excmd2(bytes.get(arg)) {
        return Some(after_argument(bytes, arg));
    }
    if bytes.get(arg) == Some(&b'/') {
        let (after, _) = skip_past(line, arg + 1, '/');
        return Some(after_argument(bytes, after));
    }
    let name_end = function_name_end(bytes, arg);
    (!line[name_end..].contains('(')).then(|| after_argument(bytes, name_end))
}

/// `ex_syntax()`: a `|` at the start of a word ends it. Deviation: Vim's
/// subcommands each read their own argument; here a word of `match`,
/// `region` or `sync` holds a pattern when it starts with a delimiter or
/// with `start=`, `skip=` or `end=`, and in the other subcommands a word
/// that starts with `"` is a comment.
fn syntax_end(line: &str, arg: usize) -> ArgEnd {
    let bytes = line.as_bytes();
    let mut p = arg;
    while bytes.get(p).is_some_and(u8::is_ascii_alphabetic) {
        p += 1;
    }
    let patterns = matches!(&line[arg..p], "match" | "region" | "sync");
    loop {
        p = skip_blanks(bytes, p);
        match bytes.get(p) {
            None => return ArgEnd::End,
            Some(b'|' | b'\n') => return ArgEnd::Bar(p),
            Some(b'"') if !patterns => return ArgEnd::Comment(p),
            Some(_) => {}
        }
        if patterns {
            for key in ["start=", "skip=", "end="] {
                if line[p..].starts_with(key) {
                    p += key.len();
                }
            }
            let delimiter = bytes
                .get(p)
                .is_some_and(|&b| !(b.is_ascii_alphanumeric() || b"_@,= \t".contains(&b)));
            if delimiter {
                match pattern_at(line, p) {
                    Some(end) => p = end,
                    None => return ArgEnd::End,
                }
            }
        }
        while !matches!(bytes.get(p), None | Some(b' ' | b'\t' | b'\n')) {
            p += char_len(bytes, p);
        }
    }
}

/// Vim's `separate_nextcmd()` over the argument at `arg` of a command with
/// the `EX_*` flags `flags`.
fn separate_nextcmd(line: &str, start: usize, arg: usize, flags: u32, name: &str) -> Piece {
    let bytes = line.as_bytes();
    let keep_ctrl_v = flags & (ex::CTRLV | ex::XFILE) != 0;
    let comments = flags & ex::NOTRLCOM == 0;
    let trim = Trim::Arg { arg, del: comments };
    let mut dropped = Vec::new();
    let mut p = if matches!(name, "vimgrep" | "vimgrepadd" | "lvimgrep" | "lvimgrepadd") {
        skip_vimgrep_pat(line, arg).unwrap_or(arg)
    } else {
        arg
    };
    while let Some(&byte) = bytes.get(p) {
        if byte == CTRL_V {
            if !keep_ctrl_v {
                dropped.push(p);
            }
            p += 1;
            if p >= bytes.len() {
                break;
            }
        } else if byte == b'`' && bytes.get(p + 1) == Some(&b'=') && flags & ex::XFILE != 0 {
            p = skip_backtick_expr(bytes, p + 2);
            if p >= bytes.len() {
                break;
            }
        } else {
            // `:@"` and `:redir @"` name a register, not a comment. `:*` is
            // a range, as 'cpoptions' has no `*`.
            let comment = byte == b'"'
                && comments
                && !(name == "@" && p == arg)
                && !(name == "redir" && p == arg + 1 && bytes.get(p - 1) == Some(&b'@'));
            if comment || byte == b'|' || byte == b'\n' {
                if p > 0 && bytes.get(p - 1) == Some(&b'\\') {
                    dropped.push(p - 1);
                } else {
                    return Piece {
                        start,
                        end: p,
                        dropped,
                        trim,
                        next: (!comment).then_some(p + 1),
                    };
                }
            }
        }
        p += char_len(bytes, p);
    }
    Piece {
        start,
        end: line.len(),
        dropped,
        trim,
        next: None,
    }
}

/// Vim's `skip_vimgrep_pat()`: past the pattern and flags of `:vimgrep`, or
/// `None` when an unclosed pattern leaves the whole argument to scan.
fn skip_vimgrep_pat(line: &str, arg: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    let first = line.get(arg..)?.chars().next()?;
    if first.is_ascii_alphanumeric() || first == '_' {
        let mut p = arg;
        while !matches!(bytes.get(p), None | Some(b' ' | b'\t')) {
            p += 1;
        }
        return Some(p);
    }
    let (mut p, closed) = skip_past(line, arg + first.len_utf8(), first);
    if !closed {
        return None;
    }
    while matches!(bytes.get(p), Some(b'g' | b'j' | b'f')) {
        p += 1;
    }
    Some(p)
}

/// Past `` `=expr` ``'s expression, at the closing backtick.
fn skip_backtick_expr(bytes: &[u8], mut p: usize) -> usize {
    while let Some(&byte) = bytes.get(p) {
        match byte {
            b'`' => break,
            b'"' => p = skip_double_quoted(bytes, p + 1),
            b'\'' => p = skip_single_quoted(bytes, p + 1),
            _ => p += 1,
        }
    }
    p
}

/// The argument at `arg` up to the first `|` without a backslash before
/// it; such a backslash is dropped.
fn first_bar(line: &str, start: usize, arg: usize) -> Piece {
    let bytes = line.as_bytes();
    let mut dropped = Vec::new();
    let mut p = arg;
    while let Some(&byte) = bytes.get(p) {
        if byte == b'|' || byte == b'\n' {
            if p > 0 && bytes.get(p - 1) == Some(&b'\\') {
                dropped.push(p - 1);
            } else {
                return Piece {
                    start,
                    end: p,
                    dropped,
                    trim: Trim::Blanks,
                    next: Some(p + 1),
                };
            }
        }
        p += char_len(bytes, p);
    }
    Piece {
        start,
        end: line.len(),
        dropped,
        trim: Trim::Blanks,
        next: None,
    }
}

/// `:!`, `:r !`, `:w !`, `:terminal`, `:global` and `:vglobal`: the rest of
/// the line, up to a newline. A backslash before a newline is dropped.
fn shell_piece(line: &str, start: usize, arg: usize) -> Piece {
    let bytes = line.as_bytes();
    let mut dropped = Vec::new();
    let mut p = arg;
    while let Some(&byte) = bytes.get(p) {
        if byte == b'\\' && bytes.get(p + 1) == Some(&b'\n') {
            dropped.push(p);
            p += 1;
        } else if byte == b'\n' {
            return Piece {
                start,
                end: p,
                dropped,
                trim: Trim::None,
                next: Some(p + 1),
            };
        }
        p += 1;
    }
    Piece {
        start,
        end: line.len(),
        dropped,
        trim: Trim::None,
        next: None,
    }
}

/// Past a pattern argument at `arg` delimited by its first character, such
/// as `/a|b/`; `arg` itself when it starts with a letter, digit or blank.
fn skip_pattern_arg(line: &str, arg: usize) -> usize {
    match line.get(arg..).and_then(|rest| rest.chars().next()) {
        Some(delim) if !delim.is_alphanumeric() && !delim.is_whitespace() => {
            skip_past(line, arg + delim.len_utf8(), delim).0
        }
        _ => arg,
    }
}

/// Past the `delim` that ends a pattern starting at `from`, with a
/// backslash escaping the character after it; `false` when the line ends
/// first.
fn skip_past(line: &str, from: usize, delim: char) -> (usize, bool) {
    let Some(rest) = line.get(from..) else {
        return (line.len(), false);
    };
    let mut chars = rest.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == delim {
            return (from + i + c.len_utf8(), true);
        }
        if c == '\\' {
            chars.next();
        }
    }
    (line.len(), false)
}

/// `:s` and `:&`, `:~`, after the name: the pattern and replacement when
/// `delim` opens them at `at`, the flags and count, then a `"` comment or a
/// `|` (`ex_substitute()`). Anything else makes Vim's `E488` and takes the
/// rest of the line.
fn substitute_piece(line: &str, start: usize, at: usize, delim: Option<char>) -> Piece {
    let bytes = line.as_bytes();
    let mut p = delim.map_or(at, |delim| {
        let (after_pattern, _) = skip_past(line, at + delim.len_utf8(), delim);
        skip_past(line, after_pattern, delim).0
    });
    if bytes.get(p) == Some(&b'&') {
        p += 1;
    }
    while bytes.get(p).is_some_and(|b| b"gcneriI#lp".contains(b)) {
        p += 1;
    }
    p = skip_blanks(bytes, p);
    while bytes.get(p).is_some_and(u8::is_ascii_digit) {
        p += 1;
    }
    p = skip_blanks(bytes, p);
    match bytes.get(p) {
        Some(b'"') => Piece {
            start,
            end: p,
            dropped: Vec::new(),
            trim: Trim::Blanks,
            next: None,
        },
        Some(b'|' | b'\n') => Piece {
            start,
            end: p,
            dropped: Vec::new(),
            trim: Trim::Blanks,
            next: Some(p + 1),
        },
        _ => Piece::rest_of_line(line, start),
    }
}

/// `:sx/{pattern}/{command}` and `:sy`, with the delimiter at `at`: the
/// command after the pattern ends the piece.
fn pattern_then_command(line: &str, start: usize, at: usize) -> Piece {
    let Some(delim) = line.get(at..).and_then(|rest| rest.chars().next()) else {
        return Piece::rest_of_line(line, start);
    };
    let (after, _) = skip_past(line, at + delim.len_utf8(), delim);
    let command = command_piece(line, skip_blanks(line.as_bytes(), after));
    if command.end <= command.start {
        return Piece {
            start,
            end: after,
            dropped: Vec::new(),
            trim: Trim::Blanks,
            next: command.next,
        };
    }
    Piece { start, ..command }
}

/// What ends a command's argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArgEnd {
    /// A `|` or newline at this byte.
    Bar(usize),
    /// A `"` comment from this byte to the end of the line.
    Comment(usize),
    /// The line.
    End,
}

/// Where the expressions of a command argument at `from` end, as
/// `eval0_retarg()` and `ends_excmd2()` find it: at a `|` outside brackets
/// that is not `||`, or at a `"` where an operator would come. For the
/// `:echo` family (`echo`) a `"` always opens another string, as in
/// `ex_echo()`.
pub(crate) fn expression_end(line: &str, from: usize, echo: bool) -> ArgEnd {
    let bytes = line.as_bytes();
    let mut p = from;
    let mut depth = 0usize;
    let mut operand_next = true;
    while let Some(&byte) = bytes.get(p) {
        match byte {
            b' ' | b'\t' => p += 1,
            b'|' if bytes.get(p + 1) == Some(&b'|') => {
                p += 2;
                operand_next = true;
            }
            b'&' if bytes.get(p + 1) == Some(&b'&') => {
                p += 2;
                operand_next = true;
            }
            b'|' if depth == 0 => return ArgEnd::Bar(p),
            b'\n' if depth == 0 => return ArgEnd::Bar(p),
            b'"' if !operand_next && !echo => return ArgEnd::Comment(p),
            b'"' => {
                p = skip_double_quoted(bytes, p + 1);
                operand_next = false;
            }
            b'\'' => {
                p = skip_single_quoted(bytes, p + 1);
                operand_next = false;
            }
            b'(' | b'[' | b'{' => {
                depth += 1;
                operand_next = true;
                p += 1;
            }
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                operand_next = false;
                p += 1;
            }
            // An option, a register and an environment variable.
            b'&' if operand_next => {
                p = skip_name(bytes, p + 1);
                operand_next = false;
            }
            b'@' if operand_next => {
                p += 1;
                if p < bytes.len() {
                    p += char_len(bytes, p);
                }
                operand_next = false;
            }
            b'$' if operand_next => {
                p = skip_name(bytes, p + 1);
                operand_next = false;
            }
            b'#' if operand_next && bytes.get(p + 1) == Some(&b'{') => p += 1,
            b if b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80 => {
                p = skip_name(bytes, p);
                // After an operand, a word is `is`, `isnot` or `in`.
                operand_next = !operand_next;
            }
            _ => {
                operand_next = true;
                p += 1;
            }
        }
    }
    ArgEnd::End
}

/// Past a name: letters, digits, `_`, `#`, other characters than ASCII, and
/// the `:` of a scope such as `g:` or `l:`.
fn skip_name(bytes: &[u8], from: usize) -> usize {
    let mut p = from;
    while let Some(&b) = bytes.get(p) {
        let scope = b == b':'
            && p == from + 1
            && bytes.get(from).is_some_and(|s| b"bgwtslav".contains(s))
            && bytes
                .get(p + 1)
                .is_some_and(|n| n.is_ascii_alphabetic() || *n == b'_');
        if !(b.is_ascii_alphanumeric() || b == b'_' || b == b'#' || b >= 0x80 || scope) {
            break;
        }
        p += 1;
    }
    p
}

/// Past the `"` that closes a string whose text starts at `from`; a
/// backslash escapes the next character.
fn skip_double_quoted(bytes: &[u8], from: usize) -> usize {
    let mut p = from;
    while let Some(&b) = bytes.get(p) {
        match b {
            b'\\' => p += 2,
            b'"' => return p + 1,
            _ => p += 1,
        }
    }
    bytes.len()
}

/// Past the `'` that closes a literal string whose text starts at `from`;
/// `''` is one quote.
fn skip_single_quoted(bytes: &[u8], from: usize) -> usize {
    let mut p = from;
    while let Some(&b) = bytes.get(p) {
        if b == b'\'' {
            if bytes.get(p + 1) == Some(&b'\'') {
                p += 2;
                continue;
            }
            return p + 1;
        }
        p += 1;
    }
    bytes.len()
}
