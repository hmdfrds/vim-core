//! How Vim reads a sourced file.
//!
//! `get_one_sourceline()` reads a line of the file, dropping the line
//! break. `getsourceline()` hands `do_cmdline()` one line at a time and
//! joins to it the continuation lines that follow, which it finds by
//! reading one line ahead. Some commands read the lines after their own
//! line as text, such as a heredoc or a function body, so that those lines
//! never run as commands.

use super::ex_split::{command_head, skip_range, vim_command_at};
use super::vim_commands::EVENTS;
use crate::errors::VimError;
use crate::grammar::types::{BlockKind, ModifierFlags};

/// A line of a sourced file, with the number of the first line of the
/// file it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) number: u32,
    pub(crate) text: String,
}

/// What reading the lines a command takes after its own line gave.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Body {
    /// The errors, each with the line Vim gives it at.
    pub(crate) errors: Vec<(u32, VimError)>,
    /// Whether the command fails, so that it does not run.
    pub(crate) fails: bool,
    /// The commands after the `|` of the line that ends a function, which
    /// run as the rest of that line.
    pub(crate) tail: Option<Line>,
}

/// Which lines a read joins to the line it reads, as Vim's
/// `getline_opt_T` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Join {
    /// None (`GETLINE_NONE`).
    None,
    /// Lines whose first non-blank is `\` (`GETLINE_CONCAT_CONT`).
    Continuation,
    /// Those and lines whose first non-blank is `|` but not `||`
    /// (`GETLINE_CONCAT_CONTBAR`).
    ContinuationAndBar,
}

/// The line break of a file, decided by its first line, as Vim's
/// MS-Windows build does (`USE_CRNL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Unknown,
    Dos,
    Unix,
}

/// A sourced file, read as Vim reads it.
#[derive(Debug)]
pub(crate) struct Script<'a> {
    text: &'a str,
    /// Where the next line starts.
    at: usize,
    /// How many lines of the file were read.
    lines_read: u32,
    format: Format,
    /// Whether W15 was given; Vim gives it once.
    warned: bool,
    /// The line read ahead, and whether reading it gave W15.
    ahead: Option<(Line, bool)>,
    /// W15, with its line, for each line taken that gave it.
    warnings: Vec<(u32, VimError)>,
}

impl<'a> Script<'a> {
    /// The file `text`. A byte order mark before it is dropped, as
    /// `do_source_ext()` drops it.
    pub(crate) fn new(text: &'a str) -> Self {
        Self {
            text: text.strip_prefix('\u{feff}').unwrap_or(text),
            at: 0,
            lines_read: 0,
            format: Format::Unknown,
            warned: false,
            ahead: None,
            warnings: Vec::new(),
        }
    }

    /// The next line to run and the continuation lines after it, joined
    /// as `getsourceline()` joins them: a line whose first non-blank is
    /// `\` adds the text after the `\`, and a `"\ ` comment line between
    /// them is dropped.
    pub(crate) fn next_line(&mut self) -> Option<Line> {
        self.read(Join::Continuation)
    }

    /// The warnings of the lines taken since the last call, each with the
    /// line it belongs to. Deviation: Vim gives W15 while it reads ahead,
    /// at the line it is fetching, and does not run that line; here W15
    /// is at the line that ends in LF alone and skips nothing.
    pub(crate) fn take_warnings(&mut self) -> Vec<(u32, VimError)> {
        std::mem::take(&mut self.warnings)
    }

    /// Reads the lines that the command `piece` of `line` takes after its
    /// own line, if it takes any.
    ///
    /// With `skipping`, the command does not run, as after an error on its
    /// line; then only the commands that `do_one_cmd()` still calls read
    /// their lines: `:let`, `:const`, `:function`, `:def` and the script
    /// commands but `:pythonx`. The errors of reading are given either way.
    ///
    /// Deviation: a command that runs always reads its lines, where Vim
    /// leaves them to run as commands after a command it refuses first,
    /// such as a script command whose interface is missing
    /// (`ex_script_ni()`), `:append` without a buffer, a `:function` whose
    /// name is wrong, or a heredoc whose marker is.
    pub(crate) fn read_body(&mut self, piece: &str, line: &Line, skipping: bool) -> Option<Body> {
        let mut body = Body::default();
        match opener(piece, skipping)? {
            Opener::Fails(error) => {
                body.errors.push((line.number, error));
                body.fails = true;
            }
            Opener::Heredoc {
                marker,
                trim,
                error,
            } => {
                if let Some(error) = error {
                    body.errors.push((line.number, error));
                    body.fails = true;
                }
                // `heredoc_get()`: with `trim`, the indent of the command's
                // line may come before the marker.
                let indent = if trim { blanks(&line.text) } else { "" };
                loop {
                    let Some(text) = self.take() else {
                        body.errors
                            .push((self.end_line(), VimError::MissingEndMarker(marker.into())));
                        break;
                    };
                    let text = text.text.as_str();
                    if text.strip_prefix(indent).unwrap_or(text) == marker {
                        break;
                    }
                }
            }
            Opener::Function { def } => body = self.function_body(def, line.number),
            // `ex_append()`: up to a line that is `.`.
            Opener::Text => {
                while let Some(text) = self.read(Join::Continuation) {
                    if text.text == "." {
                        break;
                    }
                }
            }
            // `may_get_cmd_block()`: up to a line whose first non-blank is
            // `}`.
            Opener::Block => loop {
                let Some(text) = self.read(Join::ContinuationAndBar) else {
                    body.errors.push((self.end_line(), VimError::MissingBrace));
                    break;
                };
                if blanks_skipped(&text.text).starts_with('}') {
                    break;
                }
            },
            // `ex_loadkeymap()`: the rest of the file.
            Opener::Keymap => while self.take().is_some() {},
        }
        Some(body)
    }

    /// A `:function` or `:def` body, read as `get_function_body()` reads
    /// it: up to the `:endfunction` or `:enddef` that matches, counting
    /// the functions and `{` blocks defined inside, and skipping the
    /// heredocs and `:append` text inside without looking for the end.
    /// `top` is the line of the command.
    ///
    /// Deviation: the errors Vim gives for an end that does not match or
    /// is shortened (E1151, E1152, E1065), and for text after the end
    /// (W22, E1173), are not given.
    fn function_body(&mut self, def: bool, top: u32) -> Body {
        let mut body = Body::default();
        // Each function and block open: whether it is a `:def`, and whether
        // it is a `{` block.
        let mut nesting = vec![(def, false)];
        let mut skip_until: Option<String> = None;
        let mut heredoc_indent: Option<String> = None;
        let normal = if def {
            Join::ContinuationAndBar
        } else {
            Join::Continuation
        };
        let mut join = normal;
        loop {
            let Some(line) = self.read(join) else {
                let error = match (&skip_until, nesting.last()) {
                    (Some(marker), _) => VimError::MissingHeredocEndMarker(marker.as_str().into()),
                    (None, Some((_, true))) => VimError::MissingEndBlock,
                    _ if def => VimError::MissingEnddef,
                    _ => VimError::MissingEndfunction,
                };
                body.errors.push((top, error));
                body.fails = true;
                return body;
            };
            let theline = line.text.as_str();

            if let Some(marker) = &skip_until {
                let text = match &heredoc_indent {
                    Some(indent) if theline.starts_with([' ', '\t']) => {
                        theline.strip_prefix(indent.as_str())
                    }
                    _ => Some(theline),
                };
                if text == Some(marker.as_str()) {
                    skip_until = None;
                    heredoc_indent = None;
                    join = normal;
                }
                continue;
            }

            let mut p = theline.trim_start_matches([' ', '\t', ':']);
            let (level_def, level_block) = nesting.last().copied().unwrap_or((def, false));
            let ends = if level_block {
                p.starts_with('}')
            } else {
                // A `:` after it makes a dictionary key.
                let end = if level_def { "enddef" } else { "endfunction" };
                match checkforcmd(p, end, 4) {
                    Some(after) => {
                        p = after;
                        !after.starts_with(':')
                    }
                    None => false,
                }
            };
            if ends {
                if nesting.len() == 1 {
                    let rest = p.strip_prefix(['|', '}']);
                    if !body.errors.is_empty() {
                        body.fails = true;
                    } else if let Some(rest) = rest.filter(|rest| !blanks_skipped(rest).is_empty())
                    {
                        body.tail = Some(Line {
                            number: line.number,
                            text: rest.to_owned(),
                        });
                    }
                    return body;
                }
                nesting.pop();
            } else if let Some(after) = if level_def {
                checkforcmd(p, "endfunction", 4)
            } else if def {
                checkforcmd(p, "enddef", 4)
            } else {
                None
            } {
                p = after;
            }

            // A function defined inside: `function` but not `function(`,
            // and `def` only inside `:def`.
            let is_def = p.starts_with('d');
            let defined = checkforcmd(p, "function", 2)
                .filter(|after| !after.starts_with('('))
                .or_else(|| def.then(|| checkforcmd(p, "def", 3)).flatten());
            if let Some(after) = defined {
                p = after.strip_prefix('!').map_or(after, blanks_skipped);
                p = &p[script_prefix_len(p)..];
                p = &p[name_end(p.as_bytes(), 0)..];
                if blanks_skipped(p).starts_with('(') {
                    if nesting.len() == MAX_FUNC_NESTING {
                        body.errors
                            .push((line.number, VimError::FunctionNestingTooDeep));
                    } else {
                        nesting.push((is_def, false));
                    }
                }
            }

            // A `{` block: a line ending in `=> {`, or an `:autocmd` or
            // `:command` line ending in ` {`.
            let comment = if nesting.last().is_some_and(|&(d, _)| d) {
                '#'
            } else {
                '"'
            };
            if !p.starts_with(comment) {
                let line_end = p.trim_end_matches([' ', '\t']);
                if let Some(head) = line_end.strip_suffix('{') {
                    let before = head.trim_end_matches([' ', '\t']);
                    let block = line_end.len() >= 3
                        && head.ends_with([' ', '\t'])
                        && ((before.len() >= 4 && before.ends_with("=>"))
                            || checkforcmd_noparen(p, "autocmd", 2).is_some()
                            || checkforcmd_noparen(p, "command", 3).is_some());
                    if block {
                        if nesting.len() == MAX_FUNC_NESTING {
                            body.errors
                                .push((line.number, VimError::FunctionNestingTooDeep));
                        } else {
                            nesting.push((true, true));
                        }
                    }
                }
            }

            let p = &p[skip_range(p.as_bytes(), 0)..];
            if !def && is_append(p.as_bytes()) {
                skip_until = Some(".".to_owned());
            }

            // A script heredoc: `:python << EOF` and the others, which end
            // at `.` without a marker.
            let arg = blanks_skipped(&p[word(p).len()..]);
            if let Some(rest) = arg.strip_prefix("<<").filter(|_| is_script(p.as_bytes())) {
                let mut marker = blanks_skipped(rest);
                if let Some(rest) = marker.strip_prefix("trim") {
                    marker = blanks_skipped(rest);
                    heredoc_indent = Some(blanks(theline).to_owned());
                }
                skip_until = Some(if marker.is_empty() { "." } else { word(marker) }.to_owned());
                join = Join::None;
                continue;
            }

            // A `:let`, `:var`, `:final` or `:const` heredoc; in `:def`, any
            // assignment.
            let assigned = checkforcmd(p, "let", 2)
                .or_else(|| checkforcmd(p, "var", 3))
                .or_else(|| checkforcmd(p, "final", 5))
                .or_else(|| checkforcmd(p, "const", 5))
                .or_else(|| def.then_some(p));
            if let Some(arg) = assigned {
                let arg = arg.trim_start_matches(['$', '@', '&']);
                let mut arg = blanks_skipped(&arg[name_end(arg.as_bytes(), 0)..]);
                if def {
                    if let Some(rest) = arg.strip_prefix(':') {
                        let rest = blanks_skipped(rest);
                        arg = blanks_skipped(
                            &rest[rest.find([' ', '\t', '=']).unwrap_or(rest.len())..],
                        );
                    }
                }
                if let Some(rest) = arg.strip_prefix("=<<") {
                    let mut marker = blanks_skipped(rest);
                    loop {
                        if let Some(rest) = marker.strip_prefix("trim") {
                            heredoc_indent = Some(blanks(theline).to_owned());
                            marker = blanks_skipped(rest);
                        } else if let Some(rest) = marker.strip_prefix("eval") {
                            marker = blanks_skipped(rest);
                        } else {
                            break;
                        }
                    }
                    skip_until = Some(word(marker).to_owned());
                    join = Join::None;
                }
            }
        }
    }

    /// The line where Vim stands at the end of the file: the one after the
    /// last.
    const fn end_line(&self) -> u32 {
        self.lines_read.saturating_add(1)
    }

    /// A line and the lines `join` joins to it, as `getsourceline()` reads
    /// them.
    fn read(&mut self, join: Join) -> Option<Line> {
        let mut line = self.take()?;
        if join == Join::None {
            return Some(line);
        }
        while let Some(next) = self.peek() {
            let p = blanks_skipped(&next.text);
            if let Some(rest) = p.strip_prefix('\\') {
                line.text.push_str(rest);
            } else if join == Join::ContinuationAndBar && p.starts_with('|') && !p.starts_with("||")
            {
                line.text.push(' ');
                line.text.push_str(p);
            } else if !p.starts_with("\"\\ ") {
                break;
            }
            self.take();
        }
        Some(line)
    }

    /// The line read ahead, or else the next line of the file.
    fn take(&mut self) -> Option<Line> {
        let (line, warned) = match self.ahead.take() {
            Some(ahead) => ahead,
            None => self.read_line()?,
        };
        if warned {
            self.warnings
                .push((line.number, VimError::WrongLineSeparator));
        }
        Some(line)
    }

    /// The next line, read ahead and kept for the next read.
    fn peek(&mut self) -> Option<&Line> {
        if self.ahead.is_none() {
            self.ahead = self.read_line();
        }
        self.ahead.as_ref().map(|(line, _)| line)
    }

    /// The next line of the file and whether reading it gave W15, as
    /// `get_one_sourceline()` reads it.
    ///
    /// The first line that ends in LF decides the format: when it ends in
    /// CR LF, the CR of each CR LF goes, and the first later line that
    /// ends in LF alone gives W15 and keeps the CRs of every line after
    /// it; otherwise every CR stays. A Ctrl-Z alone after the last LF of
    /// such a file is dropped. An odd number of Ctrl-V before the LF joins
    /// the next line and keeps the LF. A NUL ends the text of its line.
    ///
    /// Deviation: Unix Vim keeps every CR; the two agree on every file
    /// except one whose first line ends in CR LF.
    fn read_line(&mut self) -> Option<(Line, bool)> {
        let number = self.lines_read.saturating_add(1);
        let mut text = String::new();
        let mut have_read = false;
        let mut warned = false;
        loop {
            let rest = &self.text[self.at..];
            if rest.is_empty() {
                break;
            }
            if self.format == Format::Dos && rest == "\u{1a}" {
                self.at = self.text.len();
                break;
            }
            let (chunk, has_lf) = rest
                .find('\n')
                .map_or((rest, false), |lf| (&rest[..lf], true));
            self.at += chunk.len() + usize::from(has_lf);
            self.lines_read = self.lines_read.saturating_add(1);
            have_read = true;
            if let Some(nul) = chunk.find('\0') {
                text.push_str(&chunk[..nul]);
                break;
            }
            if !has_lf {
                text.push_str(chunk);
                break;
            }
            let mut chunk = chunk;
            let has_cr = chunk.ends_with('\r');
            if self.format == Format::Unknown {
                self.format = if has_cr { Format::Dos } else { Format::Unix };
            }
            if self.format == Format::Dos {
                if has_cr {
                    chunk = &chunk[..chunk.len() - 1];
                } else {
                    warned = !self.warned;
                    self.warned = true;
                    self.format = Format::Unix;
                }
            }
            text.push_str(chunk);
            let ctrl_v = chunk.bytes().rev().take_while(|&b| b == 0x16).count();
            if ctrl_v % 2 == 0 {
                break;
            }
            text.push('\n');
        }
        have_read.then_some((Line { number, text }, warned))
    }
}

/// A command that a sourced file runs itself: a command of a script block,
/// `:finish`, `:break` or `:continue`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Control {
    Block(BlockKind),
    Finish,
    Break,
    Continue,
}

/// A [`Control`] command as `do_one_cmd()` reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScriptCommand<'p> {
    pub(crate) control: Control,
    /// The engine's modifiers before it.
    pub(crate) modifiers: ModifierFlags,
    /// Whether a range comes before it.
    pub(crate) range: bool,
    /// Whether a `!` follows its name.
    pub(crate) bang: bool,
    /// The text after the name and the `!`, without blanks around it.
    pub(crate) argument: &'p str,
}

impl ScriptCommand<'_> {
    /// Whether the command takes an argument (`EX_EXTRA`).
    pub(crate) const fn takes_argument(&self) -> bool {
        matches!(
            self.control,
            Control::Block(
                BlockKind::If
                    | BlockKind::ElseIf
                    | BlockKind::While
                    | BlockKind::For
                    | BlockKind::Catch
            )
        )
    }
}

/// The [`Control`] command of `piece`, if it is one.
pub(crate) fn script_command(piece: &str) -> Option<ScriptCommand<'_>> {
    let head = command_head(piece)?;
    let control = match head.row.name {
        "finish" => Control::Finish,
        "break" => Control::Break,
        "continue" => Control::Continue,
        name => Control::Block(BlockKind::from_name(name)?),
    };
    let after = piece.get(head.name_end..)?;
    let argument = after.strip_prefix('!');
    Some(ScriptCommand {
        control,
        modifiers: head.modifiers,
        range: head.range,
        bang: argument.is_some(),
        argument: argument.unwrap_or(after).trim_matches([' ', '\t']),
    })
}

/// Most functions and blocks open at once in a function body, as
/// `get_function_body()` counts them.
const MAX_FUNC_NESTING: usize = 50;

/// What a command reads after its own line.
#[derive(Debug)]
enum Opener<'p> {
    /// Nothing, after the error the command gives.
    Fails(VimError),
    /// Lines up to one that is `marker`, as they are. With `trim`, the
    /// marker may follow the indent of the command's line. `error` is an
    /// error the command gives before it reads.
    Heredoc {
        marker: &'p str,
        trim: bool,
        error: Option<VimError>,
    },
    /// A `:function` or `:def` body.
    Function { def: bool },
    /// Text up to a line that is `.`.
    Text,
    /// Lines up to one whose first non-blank is `}`.
    Block,
    /// The rest of the file.
    Keymap,
}

/// What the command `piece` reads after its own line, if anything; with
/// `skipping`, only what it reads when it does not run.
fn opener(piece: &str, skipping: bool) -> Option<Opener<'_>> {
    let (row, name, name_end) = vim_command_at(piece)?;
    let after_name = &piece[name_end..];
    let arg = blanks_skipped(after_name.strip_prefix('!').unwrap_or(after_name));
    match row.name {
        "let" | "const" => heredoc_start(arg).map(|at| heredoc(&arg[at..], false)),
        "lua" | "perl" | "python" | "py3" | "python3" | "ruby" | "tcl" | "mzscheme" => {
            arg.strip_prefix("<<").map(|rest| heredoc(rest, true))
        }
        "function" | "def" => defines_function(arg).then(|| Opener::Function {
            def: row.name == "def",
        }),
        _ if skipping => None,
        "pyx" | "pythonx" => arg.strip_prefix("<<").map(|rest| heredoc(rest, true)),
        "append" | "insert" | "change" => Some(Opener::Text),
        // The parser reads `:cha` to `:change` as `:changes`.
        "changes" if name_end - name < "changes".len() => Some(Opener::Text),
        "command" => command_block(arg).then_some(Opener::Block),
        "autocmd" => autocmd_block(arg).then_some(Opener::Block),
        "loadkeymap" if matches!(arg.as_bytes().first(), None | Some(b'"')) => Some(Opener::Keymap),
        _ => None,
    }
}

/// The heredoc whose text after `<<` is `cmd`, as `heredoc_get()` reads
/// `trim`, `eval` and the marker. A script heredoc (`script`) ends at `.`
/// without a marker; a `:let` marker must not start with a lowercase
/// letter.
fn heredoc(cmd: &str, script: bool) -> Opener<'_> {
    let mut cmd = blanks_skipped(cmd);
    let mut trim = false;
    loop {
        if starts_with_word(cmd, "trim") {
            trim = true;
        } else if !starts_with_word(cmd, "eval") {
            break;
        }
        cmd = blanks_skipped(&cmd[4..]);
    }
    if cmd.is_empty() || cmd.starts_with('"') {
        return if script {
            Opener::Heredoc {
                marker: ".",
                trim,
                error: None,
            }
        } else {
            Opener::Fails(VimError::MissingMarker)
        };
    }
    let marker = word(cmd);
    let after = &cmd[marker.len()..];
    let rest = blanks_skipped(after);
    let error = if !rest.is_empty() && !rest.starts_with('"') {
        Some(VimError::TrailingCharacters(after.into()))
    } else if !script && starts_lowercase(marker) {
        Some(VimError::LowercaseMarker)
    } else {
        None
    };
    Opener::Heredoc {
        marker,
        trim,
        error,
    }
}

/// Whether `text` starts with `word` and then a blank or its end.
fn starts_with_word(text: &str, word: &str) -> bool {
    text.strip_prefix(word)
        .is_some_and(|rest| matches!(rest.as_bytes().first(), None | Some(b' ' | b'\t')))
}

/// Whether `marker` starts with a lowercase letter as `vim_islower()` sees
/// its first byte, which reads a byte that is not ASCII as Latin-1.
fn starts_lowercase(marker: &str) -> bool {
    marker
        .bytes()
        .next()
        .is_some_and(|b| char::from(b).is_lowercase())
}

/// Whether the argument `arg` of `:function` or `:def` defines a function,
/// as `define_function()` decides before it reads the body: it lists
/// functions without an argument or with a `/pattern`, and a name without
/// a `(` after it.
fn defines_function(arg: &str) -> bool {
    !matches!(
        arg.as_bytes().first(),
        None | Some(b'|' | b'"' | b'\n' | b'/')
    ) && arg.contains('(')
}

/// Whether the argument `arg` of `:command` defines a command whose
/// replacement is a `{` block, as `ex_command()` reads its attributes and
/// name before `may_get_cmd_block()`.
fn command_block(arg: &str) -> bool {
    let bytes = arg.as_bytes();
    let mut p = 0;
    while bytes.get(p) == Some(&b'-') {
        while !matches!(bytes.get(p), None | Some(b' ' | b'\t')) {
            p += 1;
        }
        p = skip_blanks(bytes, p);
    }
    if bytes.get(p).is_some_and(u8::is_ascii_alphabetic) {
        while bytes.get(p).is_some_and(u8::is_ascii_alphanumeric) {
            p += 1;
        }
    }
    block_at(arg, skip_blanks(bytes, p))
}

/// Whether the argument `arg` of `:autocmd` adds a `{` block, as
/// `do_autocmd()` reads it: an optional group, the events, the pattern,
/// `++once`, `++nested` and `nested`, then the command. Deviation: Vim
/// takes the first word for a group when such a group exists; here when it
/// names no events.
fn autocmd_block(arg: &str) -> bool {
    let bytes = arg.as_bytes();
    let mut words = Vec::new();
    let mut p = 0;
    while p < bytes.len() && bytes.get(p) != Some(&b'|') && words.len() < 8 {
        // A blank after a backslash is part of a pattern.
        let start = p;
        while p < bytes.len()
            && (!matches!(bytes.get(p), Some(b' ' | b'\t'))
                || (p > start && bytes.get(p - 1) == Some(&b'\\')))
        {
            p += 1;
        }
        words.push((start, &arg[start..p]));
        p = skip_blanks(bytes, p);
    }
    let first_command = match words.first() {
        Some((_, events)) if names_events(events) => 2,
        _ => 3,
    };
    words
        .iter()
        .skip(first_command)
        .find(|(_, word)| !matches!(*word, "++once" | "++nested" | "nested"))
        .is_some_and(|&(at, _)| block_at(arg, at))
}

/// Whether `word` names autocommand events: `*`, or event names in any
/// case joined by commas, as `find_end_event()` reads them.
fn names_events(word: &str) -> bool {
    word == "*"
        || word
            .split(',')
            .all(|name| EVENTS.iter().any(|event| event.eq_ignore_ascii_case(name)))
}

/// Whether the `{` at `at` of `arg` opens a block that
/// `may_get_cmd_block()` reads: the command ends after it, and the line
/// does not end in `}`.
fn block_at(arg: &str, at: usize) -> bool {
    let bytes = arg.as_bytes();
    bytes.get(at) == Some(&b'{')
        && matches!(
            bytes.get(skip_blanks(bytes, at + 1)),
            None | Some(b'|' | b'\n' | b'"')
        )
        && !arg.ends_with('}')
}

/// Vim's `checkforcmd()`: when `p` starts with at least `len` bytes of
/// `cmd` and then no letter or `_`, the text after it and its blanks.
fn checkforcmd<'t>(p: &'t str, cmd: &str, len: usize) -> Option<&'t str> {
    checkforcmd_opt(p, cmd, len, false)
}

/// `checkforcmd_noparen()`: as [`checkforcmd`], and not before `(` or `.`.
fn checkforcmd_noparen<'t>(p: &'t str, cmd: &str, len: usize) -> Option<&'t str> {
    checkforcmd_opt(p, cmd, len, true)
}

fn checkforcmd_opt<'t>(p: &'t str, cmd: &str, len: usize, noparen: bool) -> Option<&'t str> {
    let matched = p
        .bytes()
        .zip(cmd.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let next = p.as_bytes().get(matched).copied().unwrap_or(0);
    let ends =
        !next.is_ascii_alphabetic() && next != b'_' && !(noparen && matches!(next, b'(' | b'.'));
    (matched >= len && ends).then(|| blanks_skipped(&p[matched..]))
}

/// Whether `p` is `:append`, `:change` or `:insert` as
/// `get_function_body()` recognises them.
fn is_append(p: &[u8]) -> bool {
    let at = |i: usize| p.get(i).copied().unwrap_or(0);
    let alpha = |i: usize| at(i).is_ascii_alphabetic();
    (at(0) == b'a' && (!alpha(1) || at(1) == b'p'))
        || (at(0) == b'c'
            && (!alpha(1)
                || (at(1) == b'h'
                    && (!alpha(2)
                        || (at(2) == b'a' && (p.get(3..6) != Some(b"nge") || !alpha(6)))))))
        || (at(0) == b'i'
            && (!alpha(1)
                || (at(1) == b'n'
                    && (!alpha(2) || (at(2) == b's' && (!alpha(3) || at(3) == b'e'))))))
}

/// Whether `p` is a script command that takes a heredoc, as
/// `get_function_body()` recognises them.
fn is_script(p: &[u8]) -> bool {
    let at = |i: usize| p.get(i).copied().unwrap_or(0);
    let alpha = |i: usize| at(i).is_ascii_alphabetic();
    (at(0) == b'p'
        && at(1) == b'y'
        && (!at(2).is_ascii_alphanumeric()
            || at(2) == b't'
            || (matches!(at(2), b'3' | b'x') && !alpha(3))))
        || (at(0) == b'p' && at(1) == b'e' && (!alpha(2) || at(2) == b'r'))
        || (at(0) == b't' && at(1) == b'c' && (!alpha(2) || at(2) == b'l'))
        || (at(0) == b'l' && at(1) == b'u' && at(2) == b'a' && !alpha(3))
        || (at(0) == b'r' && at(1) == b'u' && at(2) == b'b' && (!alpha(3) || at(3) == b'y'))
        || (at(0) == b'm' && at(1) == b'z' && (!alpha(2) || at(2) == b's'))
}

/// `eval_fname_script()`: the length of a `<SID>`, `<SNR>` or `s:` prefix.
fn script_prefix_len(p: &str) -> usize {
    let bytes = p.as_bytes();
    let tag = bytes.get(1..5);
    if bytes.first() == Some(&b'<')
        && tag.is_some_and(|tag| {
            tag.eq_ignore_ascii_case(b"SID>") || tag.eq_ignore_ascii_case(b"SNR>")
        })
    {
        5
    } else if p.starts_with("s:") {
        2
    } else {
        0
    }
}

/// The text up to its first blank, as `skiptowhite()` ends it.
fn word(text: &str) -> &str {
    &text[..text.find([' ', '\t']).unwrap_or(text.len())]
}

/// The blanks `text` starts with.
fn blanks(text: &str) -> &str {
    &text[..text.len() - blanks_skipped(text).len()]
}

/// `text` after its leading blanks, as `skipwhite()` skips them.
fn blanks_skipped(text: &str) -> &str {
    text.trim_start_matches([' ', '\t'])
}

/// Where the text after `=<<` starts when the `:let` argument `arg`
/// assigns a heredoc, as `ex_let()` looks for `=<<` after the variables
/// that `skip_var_list()` skips.
pub(super) fn heredoc_start(arg: &str) -> Option<usize> {
    let bytes = arg.as_bytes();
    let end = match bytes.first() {
        Some(b'[') => bytes
            .iter()
            .position(|&b| b == b']')
            .map_or(bytes.len(), |at| at + 1),
        Some(b'@') => arg.char_indices().nth(2).map_or(bytes.len(), |(at, _)| at),
        Some(b'$' | b'&') => name_end(bytes, 1),
        _ => name_end(bytes, 0),
    };
    let at = skip_blanks(bytes, end);
    (bytes.get(at..at + 3) == Some(b"=<<")).then_some(at + 3)
}

fn skip_blanks(bytes: &[u8], mut at: usize) -> usize {
    while matches!(bytes.get(at), Some(b' ' | b'\t')) {
        at += 1;
    }
    at
}

/// Past a variable name at `from`, as `find_name_end()` skips it: letters,
/// digits, `_`, `#`, `:`, a `.` before a key, and `[...]` and `{...}`.
fn name_end(bytes: &[u8], from: usize) -> usize {
    let mut p = from;
    let mut depth = 0usize;
    while let Some(&b) = bytes.get(p) {
        match b {
            b'[' | b'{' => depth += 1,
            b']' | b'}' if depth > 0 => depth -= 1,
            _ if depth > 0 => {}
            b'.' if bytes
                .get(p + 1)
                .is_some_and(|&n| n.is_ascii_alphanumeric() || n == b'_') => {}
            _ if b.is_ascii_alphanumeric() || matches!(b, b'_' | b'#' | b':') || b >= 0x80 => {}
            _ => break,
        }
        p += 1;
    }
    p
}

#[cfg(test)]
#[path = "script_tests.rs"]
mod tests;
