//! How Vim reads a sourced file.
//!
//! `get_one_sourceline()` reads a line of the file, dropping the line
//! break. `getsourceline()` hands `do_cmdline()` one line at a time and
//! joins to it the continuation lines that follow, which it finds by
//! reading one line ahead.

use crate::errors::VimError;

/// A line of a sourced file, with the number of the first line of the
/// file it comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Line {
    pub(crate) number: u32,
    pub(crate) text: String,
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
        let mut line = self.take()?;
        while let Some(next) = self.peek() {
            let p = next.text.trim_start_matches([' ', '\t']);
            if let Some(rest) = p.strip_prefix('\\') {
                line.text.push_str(rest);
            } else if !p.starts_with("\"\\ ") {
                break;
            }
            self.take();
        }
        Some(line)
    }

    /// The warnings of the lines taken since the last call, each with the
    /// line it belongs to. Deviation: Vim gives W15 while it reads ahead,
    /// at the line it is fetching, and does not run that line; here W15
    /// is at the line that ends in LF alone and skips nothing.
    pub(crate) fn take_warnings(&mut self) -> Vec<(u32, VimError)> {
        std::mem::take(&mut self.warnings)
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
