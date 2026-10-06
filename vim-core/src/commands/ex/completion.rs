//! Ex command tab-completion.
//!
//! Provides prefix-based completion for ex commands typed in the `:` command line.
//! The command table mirrors the grammar parser's accepted names (both short and
//! long forms), kept sorted for binary-search completion.
//!
//! Also provides context-sensitive completion resolution: given the full
//! command-line text and cursor position, [`resolve_completion_context()`]
//! determines *what* to complete (command name, file path argument, setting
//! name, buffer name, etc.) and the byte range to replace.
//!
//! Setting name and value completion uses the [`VimOptions`] struct to show
//! current values and valid enum choices.
//!
//! # Layering
//!
//! Imports `std` and `primitives` (for `VimOptions`); must not import
//! `commands`, `effects`, `execution` or `dispatch`. This module is pure
//! data + lookup — no execution logic.

use std::ops::Range;

use crate::primitives::VimOptions;
use crate::state::CompletionCandidate;
use compact_str::CompactString;

/// What kind of completion is appropriate at the current cursor position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionContext {
    /// Cursor is in the command-name position.
    CommandName {
        /// The partial command name typed so far.
        prefix: String,
        /// Byte range in the original input to replace with the completion.
        replace_range: Range<usize>,
    },
    /// Cursor is in an argument position for a known command.
    Argument {
        /// The partial argument typed so far.
        arg_prefix: String,
        /// Byte range in the original input to replace with the completion.
        replace_range: Range<usize>,
        /// What kind of argument completion is appropriate.
        kind: ArgCompletionKind,
    },
    /// No meaningful completion possible.
    None,
}

/// What kind of argument completion is appropriate for a given command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgCompletionKind {
    /// File system path (`:edit`, `:write`, `:split`, etc.).
    FilePath,
    /// Buffer name (`:buffer`, `:sbuffer`).
    Buffer,
    /// Vim option/setting name (`:set`, `:setlocal`, `:setglobal`).
    Setting,
    /// Value for a specific Vim option (`:set option=value`).
    SettingValue {
        /// The option name whose value is being completed.
        option: String,
    },
    /// Host action name (`:action`).
    Action,
}

/// Sorted table of all ex command names recognised by the grammar parser.
///
/// Includes canonical names (full forms) and minimum abbreviations. The parser
/// uses bracket-abbreviation matching (`d[elete]` accepts `d`, `de`, `del`, etc.)
/// so only the minimum prefix and full name need to appear here for completion.
///
/// Kept in alphabetical order so binary-search partition can be used for
/// prefix matching.
///
/// **Maintenance rule:** when a new ex command is added to `grammar::ex_parser`,
/// add its minimum abbreviation and full name here too.
const COMMANDS: &[&str] = &[
    "action",
    "actionl",
    "actionlist",
    "ali",
    "align",
    "b",
    "bd",
    "bdelete",
    "bf",
    "bfirst",
    "bl",
    "blast",
    "bn",
    "bnext",
    "bp",
    "bprevious",
    "bre",
    "brewind",
    "bufdo",
    "buffer",
    "buffers",
    "bw",
    "bwipeout",
    "cc",
    "ce",
    "center",
    "centre",
    "cha",
    "changes",
    "cl",
    "clist",
    "clo",
    "close",
    "cn",
    "cnext",
    "co",
    "copy",
    "cp",
    "cprev",
    "cprevious",
    "cq",
    "cquit",
    "d",
    "delete",
    "delm",
    "delmarks",
    "di",
    "display",
    "e",
    "ea",
    "earlier",
    "ec",
    "echo",
    "edit",
    "files",
    "fo",
    "fold",
    "foldc",
    "foldclose",
    "foldo",
    "foldopen",
    "im",
    "imap",
    "ino",
    "inoremap",
    "iu",
    "iunmap",
    "j",
    "join",
    "ju",
    "jumps",
    "kee",
    "keep",
    "lat",
    "later",
    "le",
    "left",
    "let",
    "ls",
    "m",
    "map",
    "mar",
    "marks",
    "mes",
    "messages",
    "move",
    "new",
    "nm",
    "nmap",
    "nn",
    "nnoremap",
    "no",
    "noh",
    "nohlsearch",
    "noremap",
    "norm",
    "normal",
    "nu",
    "number",
    "nun",
    "nunmap",
    "om",
    "omap",
    "on",
    "only",
    "ono",
    "onoremap",
    "ou",
    "ounmap",
    "pu",
    "put",
    "q",
    "qa",
    "qall",
    "quit",
    "r",
    "read",
    "reg",
    "registers",
    "remo",
    "remove",
    "ret",
    "retab",
    "ri",
    "right",
    "rot",
    "rotate",
    "se",
    "sel",
    "select",
    "set",
    "seth",
    "sethandler",
    "so",
    "sor",
    "sort",
    "source",
    "sp",
    "split",
    "t",
    "tabc",
    "tabclose",
    "tabdo",
    "tabe",
    "tabedit",
    "tabn",
    "tabnew",
    "tabnext",
    "tabp",
    "tabprevious",
    "tri",
    "trim",
    "undoj",
    "undojoin",
    "undol",
    "undolist",
    "undot",
    "undotree",
    "unm",
    "unmap",
    "up",
    "update",
    "vm",
    "vmap",
    "vn",
    "vne",
    "vnew",
    "vnoremap",
    "vs",
    "vsplit",
    "vu",
    "vunmap",
    "w",
    "wa",
    "wall",
    "windo",
    "wq",
    "wqa",
    "wqall",
    "write",
    "x",
    "xa",
    "xall",
    "xit",
    "y",
    "yank",
];

/// Complete an ex command prefix.
///
/// Returns all command names from the internal `COMMANDS` table that start with `prefix`.
/// An empty prefix returns all commands.
///
/// # Examples
///
/// ```
/// use vim_core::commands::ex::completion::complete_ex_command;
///
/// let results = complete_ex_command("w");
/// assert!(results.contains(&"write"));
/// assert!(results.contains(&"wq"));
///
/// let empty = complete_ex_command("zzzzz");
/// assert!(empty.is_empty());
/// ```
#[must_use]
pub fn complete_ex_command(prefix: &str) -> Vec<&'static str> {
    if prefix.is_empty() {
        return COMMANDS.to_vec();
    }

    // Since COMMANDS is sorted, use partition_point for an efficient scan:
    // find the first command >= prefix, then collect while starts_with holds.
    let start = COMMANDS.partition_point(|cmd| *cmd < prefix);
    COMMANDS
        .get(start..)
        .unwrap_or_default()
        .iter()
        .copied()
        .take_while(|cmd| cmd.starts_with(prefix))
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════
// Setting name and value completion
// ═══════════════════════════════════════════════════════════════════════════

/// Sorted table of all option names the engine supports.
///
/// Includes both canonical long names and Vim abbreviations. Kept in
/// alphabetical order so binary-search prefix matching can be used (same
/// pattern as [`COMMANDS`]).
///
/// Boolean "no{option}" forms are synthesized dynamically in
/// [`complete_setting_name()`] — they do not appear in this table.
///
/// **Maintenance rule:** when a new option is added to
/// [`VimOptions`](crate::primitives::VimOptions), add both the canonical
/// name and any abbreviation here.
const OPTION_NAMES: &[&str] = &[
    "ai",
    "autoindent",
    "backspace",
    "belloff",
    "bo",
    "bs",
    "cb",
    "clipboard",
    "com",
    "comments",
    "commentstring",
    "et",
    "expandtab",
    "fo",
    "formatoptions",
    "gd",
    "gdefault",
    "hls",
    "hlsearch",
    "ic",
    "icm",
    "ignorecase",
    "inccommand",
    "incsearch",
    "is",
    "isk",
    "iskeyword",
    "langmap",
    "langremap",
    "lmap",
    "lrm",
    "mlf",
    "mlfr",
    "multilinefind",
    "multilinefindrange",
    "nu",
    "number",
    "relativenumber",
    "rnu",
    "scrolloff",
    "scs",
    "se", // abbreviation for "selection" (note: also an ex command)
    "sel",
    "selection",
    "shiftwidth",
    "si",
    "sidescrolloff",
    "siso",
    "smartcase",
    "smartindent",
    "so",
    "softtabstop",
    "sts",
    "sw",
    "tabstop",
    "textwidth",
    "timeoutlen",
    "tm",
    "ts",
    "tw",
    "uagm",
    "ul",
    "undoautogroupms",
    "undolevels",
    "ve",
    "virtualedit",
    "visualstar",
    "whichwrap",
    "wrapscan",
    "ws",
    "ww",
];

/// Mapping from canonical option names to short human-readable descriptions.
///
/// Only canonical (long) names are listed here; abbreviations resolve to
/// their canonical name via [`canonical_option_name()`] before lookup.
const OPTION_DESCRIPTIONS: &[(&str, &str)] = &[
    ("autoindent", "Copy indent from current line on new line"),
    ("backspace", "What backspace can delete over"),
    ("belloff", "Suppress bell: \"\" or \"all\""),
    ("clipboard", "Clipboard integration mode"),
    ("comments", "Comment leaders recognized when formatting"),
    ("commentstring", "Comment string format (e.g. \"// %s\")"),
    ("expandtab", "Expand tabs to spaces"),
    ("formatoptions", "Automatic formatting flags (e.g. \"tcq\")"),
    ("gdefault", ":s substitutes globally by default"),
    ("hlsearch", "Highlight all search matches"),
    ("ignorecase", "Case-insensitive search"),
    ("inccommand", "Live substitute preview mode"),
    ("incsearch", "Show matches incrementally while typing"),
    ("iskeyword", "Characters that form keywords"),
    ("langmap", "Keyboard layout translation map"),
    ("langremap", "Apply langmap to mapping output"),
    ("multilinefind", "Allow f/F/t/T to cross line boundaries"),
    (
        "multilinefindrange",
        "Max lines to cross for multiline find",
    ),
    ("number", "Show absolute line numbers"),
    ("relativenumber", "Show relative line numbers"),
    ("scrolloff", "Min lines above/below cursor"),
    ("selection", "Selection behavior: inclusive/exclusive"),
    ("shiftwidth", "Indent/outdent width in columns"),
    ("sidescrolloff", "Min columns left/right of cursor"),
    (
        "smartcase",
        "Override ignorecase when pattern has uppercase",
    ),
    ("smartindent", "Smart C-like autoindent"),
    (
        "softtabstop",
        "Columns for Tab in insert mode (0=tabstop, -1=shiftwidth)",
    ),
    ("tabstop", "Tab stop width in columns"),
    ("textwidth", "Max line width for formatting (0 = no limit)"),
    ("timeoutlen", "Mapping timeout in milliseconds"),
    (
        "undoautogroupms",
        "Time-based undo auto-grouping window in ms (-1 = disabled)",
    ),
    ("undolevels", "Max undo levels (-1 = unlimited)"),
    ("virtualedit", "Where virtual editing is allowed"),
    ("visualstar", "Visual mode * and # search selected text"),
    ("whichwrap", "Keys that wrap to next/prev line"),
    ("wrapscan", "Searches wrap around end of file"),
];

/// Map an option abbreviation to its canonical (long) name.
///
/// Returns `None` for names that are already canonical or unknown.
fn canonical_option_name(name: &str) -> &str {
    match name {
        "ai" => "autoindent",
        "bo" => "belloff",
        "bs" => "backspace",
        "cb" => "clipboard",
        "com" => "comments",
        "et" => "expandtab",
        "fo" => "formatoptions",
        "gd" => "gdefault",
        "hls" => "hlsearch",
        "ic" => "ignorecase",
        "icm" => "inccommand",
        "is" => "incsearch",
        "isk" => "iskeyword",
        "lmap" => "langmap",
        "lrm" => "langremap",
        "mlf" => "multilinefind",
        "mlfr" => "multilinefindrange",
        "nu" => "number",
        "rnu" => "relativenumber",
        "scs" => "smartcase",
        "se" | "sel" => "selection",
        "si" => "smartindent",
        "siso" => "sidescrolloff",
        "so" => "scrolloff",
        "sts" => "softtabstop",
        "sw" => "shiftwidth",
        "tm" => "timeoutlen",
        "ts" => "tabstop",
        "tw" => "textwidth",
        "uagm" => "undoautogroupms",
        "ul" => "undolevels",
        "ve" => "virtualedit",
        "ws" => "wrapscan",
        "ww" => "whichwrap",
        other => other,
    }
}

/// Look up the human-readable description for an option (by canonical name).
fn option_description(canonical: &str) -> Option<&'static str> {
    // OPTION_DESCRIPTIONS is small enough that linear scan is fine.
    OPTION_DESCRIPTIONS.iter().find_map(|(name, desc)| {
        if *name == canonical {
            Some(*desc)
        } else {
            None
        }
    })
}

/// Boolean option names (canonical only). Used to determine which options
/// support `no{name}` prefixing and to identify option type for value display.
const BOOL_OPTIONS: &[&str] = &[
    "autoindent",
    "expandtab",
    "gdefault",
    "hlsearch",
    "ignorecase",
    "incsearch",
    "langremap",
    "multilinefind",
    "number",
    "relativenumber",
    "smartcase",
    "smartindent",
    "wrapscan",
];

/// Numeric option names (canonical only). These have integer values.
const NUMERIC_OPTIONS: &[&str] = &[
    "multilinefindrange",
    "scrolloff",
    "shiftwidth",
    "sidescrolloff",
    "softtabstop",
    "tabstop",
    "textwidth",
    "timeoutlen",
    "undoautogroupms",
    "undolevels",
];

/// Whether the canonical option name is a boolean option.
fn is_bool_option(canonical: &str) -> bool {
    BOOL_OPTIONS.contains(&canonical)
}

/// Whether the canonical option name is a numeric option.
fn is_numeric_option(canonical: &str) -> bool {
    NUMERIC_OPTIONS.contains(&canonical)
}

/// Format the current value of an option as a short display string.
///
/// - Boolean: `[on]` / `[off]`
/// - Numeric: `[= 8]`
/// - String/enum: `[= value]` or `[= (empty)]`
fn format_option_value(name: &str, options: &VimOptions) -> String {
    let canonical = canonical_option_name(name);
    if is_bool_option(canonical) {
        let val = get_bool_value(canonical, options);
        if val {
            "[on]".to_owned()
        } else {
            "[off]".to_owned()
        }
    } else if is_numeric_option(canonical) {
        let val = get_numeric_value(canonical, options);
        format!("[= {val}]")
    } else {
        let val = get_string_value(canonical, options);
        if val.is_empty() {
            "[= (empty)]".to_owned()
        } else {
            format!("[= {val}]")
        }
    }
}

/// Get the boolean value of a known boolean option.
fn get_bool_value(canonical: &str, options: &VimOptions) -> bool {
    match canonical {
        "autoindent" => options.autoindent(),
        "expandtab" => options.expandtab(),
        "gdefault" => options.gdefault(),
        "hlsearch" => options.hlsearch(),
        "ignorecase" => options.ignorecase(),
        "incsearch" => options.incsearch(),
        "langremap" => options.langremap(),
        "number" => options.number(),
        "relativenumber" => options.relativenumber(),
        "smartcase" => options.smartcase(),
        "smartindent" => options.smartindent(),
        "wrapscan" => options.wrapscan(),
        _ => false,
    }
}

/// Get the numeric value of a known numeric option as a string.
fn get_numeric_value(canonical: &str, options: &VimOptions) -> String {
    match canonical {
        "scrolloff" => options.scrolloff().to_string(),
        "shiftwidth" => options.shiftwidth().to_string(),
        "sidescrolloff" => options.sidescrolloff().to_string(),
        "softtabstop" => options.softtabstop().to_string(),
        "tabstop" => options.tabstop().to_string(),
        "textwidth" => options.textwidth().to_string(),
        "timeoutlen" => options.timeoutlen_ms().to_string(),
        "undoautogroupms" => options
            .undo_auto_group_ms()
            .map_or_else(|| "-1".to_owned(), |v| v.to_string()),
        "undolevels" => options
            .undolevels()
            .map_or_else(|| "-1".to_owned(), |v| v.to_string()),
        _ => "?".to_owned(),
    }
}

/// Get the string value of a known string/enum option.
fn get_string_value(canonical: &str, options: &VimOptions) -> String {
    match canonical {
        "backspace" => options.backspace().to_owned(),
        "belloff" => {
            if options.belloff() {
                "all".to_owned()
            } else {
                String::new()
            }
        }
        "clipboard" => options.clipboard().to_owned(),
        "comments" => options.comments().to_owned(),
        "commentstring" => options.commentstring().to_owned(),
        "formatoptions" => options.formatoptions().to_owned(),
        "inccommand" => options.inccommand().to_owned(),
        "iskeyword" => options.iskeyword().to_owned(),
        "langmap" => options.langmap().to_owned(),
        "selection" => options.selection().to_owned(),
        "virtualedit" => options.virtualedit().to_owned(),
        "whichwrap" => options.whichwrap().to_owned(),
        _ => String::new(),
    }
}

/// Complete a setting name prefix.
///
/// Returns [`CompletionCandidate`] entries for all option names that start
/// with `prefix`, including dynamically generated `no{option}` forms for
/// boolean options when `prefix` starts with `"no"`.
///
/// Each candidate includes:
/// - `text`: the option name
/// - `description`: current value formatted as `[on]`/`[off]`/`[= N]`/`[= val]`
/// - `detail`: human-readable description from the internal `OPTION_DESCRIPTIONS` table
///
/// # Examples
///
/// ```
/// # use vim_core::primitives::VimOptions;
/// # use vim_core::commands::ex::completion::complete_setting_name;
/// let opts = VimOptions::default();
/// let results = complete_setting_name("scr", &opts);
/// assert!(results.iter().any(|c| c.text.as_str() == "scrolloff"));
/// ```
#[must_use]
pub fn complete_setting_name(prefix: &str, options: &VimOptions) -> Vec<CompletionCandidate> {
    let mut candidates = Vec::new();

    if prefix.is_empty() {
        // Empty prefix: return all options
        for &name in OPTION_NAMES {
            candidates.push(make_setting_candidate(name, options));
        }
        // Also add no{option} forms for all boolean options
        for &bool_name in BOOL_OPTIONS {
            let no_name = format!("no{bool_name}");
            candidates.push(make_no_setting_candidate(&no_name, bool_name, options));
        }
        candidates.sort_by(|a, b| a.text.cmp(&b.text));
        return candidates;
    }

    // Binary-search prefix matching on the sorted OPTION_NAMES table
    let start = OPTION_NAMES.partition_point(|name| *name < prefix);
    for &name in OPTION_NAMES.get(start..).unwrap_or_default() {
        if !name.starts_with(prefix) {
            break;
        }
        candidates.push(make_setting_candidate(name, options));
    }

    // Handle "no{option}" prefix matching for boolean options.
    // If the prefix starts with "no", try matching the remainder against
    // boolean option names.
    if let Some(rest) = prefix.strip_prefix("no") {
        for &bool_name in BOOL_OPTIONS {
            if bool_name.starts_with(rest) {
                let no_name = format!("no{bool_name}");
                // Avoid duplicates if "no..." already matched a real option name above
                if !candidates.iter().any(|c| c.text.as_str() == no_name) {
                    candidates.push(make_no_setting_candidate(&no_name, bool_name, options));
                }
            }
        }
    }

    candidates
}

/// Build a `CompletionCandidate` for a regular option name.
fn make_setting_candidate(name: &str, options: &VimOptions) -> CompletionCandidate {
    let canonical = canonical_option_name(name);
    let value_display = format_option_value(name, options);
    let detail = option_description(canonical).map(CompactString::from);

    CompletionCandidate {
        text: CompactString::from(name),
        description: Some(CompactString::from(value_display)),
        detail,
    }
}

/// Build a `CompletionCandidate` for a `no{option}` boolean form.
fn make_no_setting_candidate(
    no_name: &str,
    bool_name: &str,
    options: &VimOptions,
) -> CompletionCandidate {
    let current = get_bool_value(bool_name, options);
    let value_display = if current {
        "[on -> off]".to_owned()
    } else {
        "[already off]".to_owned()
    };
    let detail =
        option_description(bool_name).map(|d| CompactString::from(format!("Disable: {d}")));

    CompletionCandidate {
        text: CompactString::from(no_name),
        description: Some(CompactString::from(value_display)),
        detail,
    }
}

/// Complete a setting value for a known option.
///
/// For enum options (like `selection`, `inccommand`), returns the valid
/// values with the current value marked `[current]`. For boolean options,
/// returns `{name}` and `no{name}`. For numeric/string/unknown options,
/// returns empty (no meaningful value completion).
///
/// # Examples
///
/// ```
/// # use vim_core::primitives::VimOptions;
/// # use vim_core::commands::ex::completion::complete_setting_value;
/// let opts = VimOptions::default();
/// let results = complete_setting_value("selection", "", &opts);
/// assert!(results.iter().any(|c| c.text.as_str() == "inclusive"));
/// assert!(results.iter().any(|c| c.text.as_str() == "exclusive"));
/// ```
#[must_use]
pub fn complete_setting_value(
    option: &str,
    prefix: &str,
    options: &VimOptions,
) -> Vec<CompletionCandidate> {
    let canonical = canonical_option_name(option);

    match canonical {
        "selection" => {
            let current = options.selection();
            enum_value_candidates(&["inclusive", "exclusive", "old"], current, prefix)
        }
        "inccommand" => {
            let current = options.inccommand();
            // inccommand uses "" for off, but we show "nosplit", "split", ""
            enum_value_candidates(&["nosplit", "split", ""], current, prefix)
        }
        "clipboard" => {
            let current = options.clipboard();
            enum_value_candidates(&["unnamed", "unnamedplus", ""], current, prefix)
        }
        "virtualedit" => {
            let current = options.virtualedit();
            enum_value_candidates(&["", "block", "insert", "all", "onemore"], current, prefix)
        }
        "belloff" => {
            let current = if options.belloff() { "all" } else { "" };
            enum_value_candidates(&["", "all"], current, prefix)
        }
        _ if is_bool_option(canonical) => {
            // For boolean options set via `:set option=`, offer the name
            // and no{name} as values (though this form is unusual — normally
            // you just use `:set name` / `:set noname`).
            let current_val = get_bool_value(canonical, options);
            let name_display = if current_val { "[current]" } else { "" };
            let no_display = if current_val { "" } else { "[current]" };

            let mut candidates = Vec::new();
            let name_text = canonical;
            let no_text = format!("no{canonical}");

            if name_text.starts_with(prefix) {
                candidates.push(CompletionCandidate {
                    text: CompactString::from(name_text),
                    description: if name_display.is_empty() {
                        None
                    } else {
                        Some(CompactString::from(name_display))
                    },
                    detail: None,
                });
            }
            if no_text.starts_with(prefix) {
                candidates.push(CompletionCandidate {
                    text: CompactString::from(no_text),
                    description: if no_display.is_empty() {
                        None
                    } else {
                        Some(CompactString::from(no_display))
                    },
                    detail: None,
                });
            }
            candidates
        }
        // Numeric and string options: no meaningful value completion
        _ => Vec::new(),
    }
}

/// Build completion candidates for a set of enum values.
///
/// Filters by `prefix` and marks the current value with `[current]`.
fn enum_value_candidates(values: &[&str], current: &str, prefix: &str) -> Vec<CompletionCandidate> {
    values
        .iter()
        .filter(|&&v| v.starts_with(prefix))
        .map(|&v| {
            let is_current = v == current;
            let display_text = if v.is_empty() { "(empty)" } else { v };
            CompletionCandidate {
                text: CompactString::from(v),
                description: if is_current {
                    Some(CompactString::from("[current]"))
                } else {
                    None
                },
                detail: Some(CompactString::from(display_text)),
            }
        })
        .collect()
}

/// Determine the completion context from command-line text and cursor position.
///
/// Parses the text up to `cursor` to figure out whether the user is typing a
/// command name or an argument, and if it's an argument, what *kind* of
/// completion is appropriate (file path, buffer, setting, etc.).
///
/// The returned [`CompletionContext`] includes a `replace_range` that tells
/// the caller which byte span of `input` should be replaced by the chosen
/// completion candidate.
///
/// # Examples
///
/// ```
/// use vim_core::commands::ex::completion::{resolve_completion_context, CompletionContext};
///
/// // Command-name position
/// let ctx = resolve_completion_context("se", 2);
/// assert!(matches!(ctx, CompletionContext::CommandName { ref prefix, .. } if prefix == "se"));
///
/// // Argument position (file path)
/// let ctx = resolve_completion_context("edit src/", 9);
/// assert!(matches!(ctx, CompletionContext::Argument { .. }));
/// ```
#[must_use]
pub fn resolve_completion_context(input: &str, cursor: usize) -> CompletionContext {
    let text = &input[..cursor.min(input.len())];
    let trimmed_start = text.len() - text.trim_start().len();
    let trimmed = text.trim_start();

    if trimmed.is_empty() {
        return CompletionContext::None;
    }

    // Find the first space to separate command from arguments.
    let first_space = trimmed.find(' ');

    match first_space {
        None => {
            // No space — cursor is still in the command name.
            CompletionContext::CommandName {
                prefix: trimmed.to_owned(),
                replace_range: trimmed_start..cursor,
            }
        }
        Some(space_idx) => {
            let cmd_text = &trimmed[..space_idx];
            let arg_start_offset = trimmed_start + space_idx + 1;
            let arg_text = if cursor > arg_start_offset {
                &input[arg_start_offset..cursor]
            } else {
                ""
            };

            match arg_completion_kind(cmd_text) {
                Some(ArgCompletionKind::Setting) => {
                    // Check for `:set option=value` sub-context.
                    if let Some(eq_pos) = arg_text.find('=') {
                        let option_name = arg_text[..eq_pos].trim().to_owned();
                        let value_prefix = arg_text[eq_pos + 1..].to_owned();
                        CompletionContext::Argument {
                            arg_prefix: value_prefix,
                            replace_range: (arg_start_offset + eq_pos + 1)..cursor,
                            kind: ArgCompletionKind::SettingValue {
                                option: option_name,
                            },
                        }
                    } else {
                        CompletionContext::Argument {
                            arg_prefix: arg_text.to_owned(),
                            replace_range: arg_start_offset..cursor,
                            kind: ArgCompletionKind::Setting,
                        }
                    }
                }
                Some(kind) => CompletionContext::Argument {
                    arg_prefix: arg_text.to_owned(),
                    replace_range: arg_start_offset..cursor,
                    kind,
                },
                None => CompletionContext::None,
            }
        }
    }
}

/// Map a command name (or abbreviation) to its argument completion kind.
///
/// Uses Vim's abbreviation rules: `"e"` matches `"edit"`, `"sp"` matches
/// `"split"`, etc. Commands not recognised or that don't take completable
/// arguments return `None`.
fn arg_completion_kind(cmd: &str) -> Option<ArgCompletionKind> {
    if matches_cmd(cmd, "e", "edit")
        || matches_cmd(cmd, "sp", "split")
        || matches_cmd(cmd, "vs", "vsplit")
        || matches_cmd(cmd, "tabe", "tabedit")
        || matches_cmd(cmd, "w", "write")
        || matches_cmd(cmd, "sav", "saveas")
        || matches_cmd(cmd, "r", "read")
        || matches_cmd(cmd, "so", "source")
    {
        Some(ArgCompletionKind::FilePath)
    } else if matches_cmd(cmd, "b", "buffer")
        || matches_cmd(cmd, "sb", "sbuffer")
        || matches_cmd(cmd, "bd", "bdelete")
        || matches_cmd(cmd, "bw", "bwipeout")
    {
        Some(ArgCompletionKind::Buffer)
    } else if matches_cmd(cmd, "se", "set")
        || matches_cmd(cmd, "setl", "setlocal")
        || matches_cmd(cmd, "setg", "setglobal")
    {
        Some(ArgCompletionKind::Setting)
    } else if matches_cmd(cmd, "action", "action") {
        Some(ArgCompletionKind::Action)
    } else {
        None
    }
}

/// Check if `name` matches the abbreviation range `[min..full]`.
///
/// Vim commands have a minimum abbreviation (e.g. `"e"` for `"edit"`) and
/// any prefix from the minimum through the full name is accepted. This
/// function returns `true` when `name` is a valid prefix in that range.
fn matches_cmd(name: &str, min: &str, full: &str) -> bool {
    let n = name.len();
    n >= min.len() && n <= full.len() && name.eq_ignore_ascii_case(&full[..n])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_table_is_sorted() {
        for pair in COMMANDS.windows(2) {
            assert!(
                pair[0] < pair[1],
                "COMMANDS not sorted: {:?} >= {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn complete_w_returns_write_and_wq() {
        let results = complete_ex_command("w");
        assert!(results.contains(&"w"));
        assert!(results.contains(&"wq"));
        assert!(results.contains(&"write"));
    }

    #[test]
    fn complete_empty_returns_all() {
        let results = complete_ex_command("");
        assert_eq!(results.len(), COMMANDS.len());
    }

    #[test]
    fn complete_nonexistent_returns_empty() {
        let results = complete_ex_command("zzzzz");
        assert!(results.is_empty());
    }

    #[test]
    fn complete_exact_match() {
        let results = complete_ex_command("quit");
        assert_eq!(results, vec!["quit"]);
    }

    #[test]
    fn complete_single_char_d() {
        let results = complete_ex_command("d");
        assert!(results.contains(&"d"));
        assert!(results.contains(&"delete"));
        assert!(!results.contains(&"edit"), "edit does not start with d");
    }

    #[test]
    fn complete_no_prefix_includes_substitution_aliases() {
        let all = complete_ex_command("");
        // s and g are handled by the parser's delimiter-based detection,
        // not named commands — so they should NOT be in this table.
        assert!(!all.contains(&"s"));
        assert!(!all.contains(&"g"));
        assert!(!all.contains(&"v"));
    }

    #[test]
    fn complete_sort() {
        let results = complete_ex_command("so");
        assert!(results.contains(&"sor"));
        assert!(results.contains(&"sort"));
        assert!(results.contains(&"source"));
    }

    #[test]
    fn complete_n_returns_noh_and_norm_variants() {
        let results = complete_ex_command("n");
        assert!(results.contains(&"noh"));
        assert!(results.contains(&"nohlsearch"));
        assert!(results.contains(&"norm"));
        assert!(results.contains(&"normal"));
        assert!(results.contains(&"nu"));
        assert!(results.contains(&"number"));
    }

    #[test]
    fn complete_includes_new_commands() {
        let all = complete_ex_command("");
        // Commands that were missing from the old table
        assert!(all.contains(&"earlier"));
        assert!(all.contains(&"later"));
        assert!(all.contains(&"echo"));
        assert!(all.contains(&"source"));
        assert!(all.contains(&"action"));
        assert!(all.contains(&"sethandler"));
        assert!(all.contains(&"undolist"));
        assert!(all.contains(&"undotree"));
        assert!(all.contains(&"buffer"));
        assert!(all.contains(&"bnext"));
        assert!(all.contains(&"bprevious"));
        assert!(all.contains(&"blast"));
        assert!(all.contains(&"tabnew"));
        assert!(all.contains(&"tabnext"));
        assert!(all.contains(&"tabclose"));
        assert!(all.contains(&"number"));
        assert!(all.contains(&"let"));
        assert!(all.contains(&"messages"));
        // Window- and buffer-iteration commands.
        assert!(all.contains(&"windo"));
        assert!(all.contains(&"bufdo"));
        assert!(all.contains(&"tabdo"));
        assert!(all.contains(&"undojoin"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // resolve_completion_context tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn context_command_name() {
        let ctx = resolve_completion_context("se", 2);
        assert!(matches!(ctx, CompletionContext::CommandName { ref prefix, .. } if prefix == "se"));
    }

    #[test]
    fn context_command_name_with_whitespace() {
        let ctx = resolve_completion_context("  edi", 5);
        assert!(
            matches!(ctx, CompletionContext::CommandName { ref prefix, .. } if prefix == "edi")
        );
    }

    #[test]
    fn context_command_name_replace_range() {
        let ctx = resolve_completion_context("se", 2);
        match ctx {
            CompletionContext::CommandName { replace_range, .. } => {
                assert_eq!(replace_range, 0..2);
            }
            other => panic!("expected CommandName, got {other:?}"),
        }
    }

    #[test]
    fn context_command_name_replace_range_with_whitespace() {
        let ctx = resolve_completion_context("  edi", 5);
        match ctx {
            CompletionContext::CommandName { replace_range, .. } => {
                assert_eq!(replace_range, 2..5);
            }
            other => panic!("expected CommandName, got {other:?}"),
        }
    }

    #[test]
    fn context_file_path() {
        let ctx = resolve_completion_context("edit src/", 9);
        assert!(matches!(ctx, CompletionContext::Argument {
                kind: ArgCompletionKind::FilePath,
                ref arg_prefix, ..
            } if arg_prefix == "src/"));
    }

    #[test]
    fn context_file_path_abbreviated_command() {
        let ctx = resolve_completion_context("e src/", 6);
        assert!(matches!(ctx, CompletionContext::Argument {
                kind: ArgCompletionKind::FilePath,
                ref arg_prefix, ..
            } if arg_prefix == "src/"));
    }

    #[test]
    fn context_buffer() {
        let ctx = resolve_completion_context("b foo", 5);
        assert!(matches!(ctx, CompletionContext::Argument {
                kind: ArgCompletionKind::Buffer,
                ref arg_prefix, ..
            } if arg_prefix == "foo"));
    }

    #[test]
    fn context_setting() {
        let ctx = resolve_completion_context("set scr", 7);
        assert!(matches!(ctx, CompletionContext::Argument {
                kind: ArgCompletionKind::Setting,
                ref arg_prefix, ..
            } if arg_prefix == "scr"));
    }

    #[test]
    fn context_setting_abbreviated() {
        let ctx = resolve_completion_context("se scr", 6);
        assert!(matches!(ctx, CompletionContext::Argument {
                kind: ArgCompletionKind::Setting,
                ref arg_prefix, ..
            } if arg_prefix == "scr"));
    }

    #[test]
    fn context_setting_value() {
        let ctx = resolve_completion_context("set selection=ex", 16);
        assert!(matches!(ctx, CompletionContext::Argument {
                kind: ArgCompletionKind::SettingValue { ref option },
                ref arg_prefix, ..
            } if option == "selection" && arg_prefix == "ex"));
    }

    #[test]
    fn context_setting_value_replace_range() {
        let ctx = resolve_completion_context("set selection=ex", 16);
        match ctx {
            CompletionContext::Argument { replace_range, .. } => {
                assert_eq!(replace_range, 14..16);
            }
            other => panic!("expected Argument, got {other:?}"),
        }
    }

    #[test]
    fn context_unknown_command() {
        let ctx = resolve_completion_context("foobar arg", 10);
        assert!(matches!(ctx, CompletionContext::None));
    }

    #[test]
    fn context_empty() {
        let ctx = resolve_completion_context("", 0);
        assert!(matches!(ctx, CompletionContext::None));
    }

    #[test]
    fn context_command_after_space_no_arg() {
        let ctx = resolve_completion_context("edit ", 5);
        assert!(matches!(ctx, CompletionContext::Argument {
                kind: ArgCompletionKind::FilePath,
                ref arg_prefix, ..
            } if arg_prefix.is_empty()));
    }

    #[test]
    fn context_write_is_file() {
        let ctx = resolve_completion_context("w foo", 5);
        assert!(matches!(
            ctx,
            CompletionContext::Argument {
                kind: ArgCompletionKind::FilePath,
                ..
            }
        ));
    }

    #[test]
    fn context_split_is_file() {
        let ctx = resolve_completion_context("sp foo", 6);
        assert!(matches!(
            ctx,
            CompletionContext::Argument {
                kind: ArgCompletionKind::FilePath,
                ..
            }
        ));
    }

    #[test]
    fn context_vsplit_is_file() {
        let ctx = resolve_completion_context("vs foo", 6);
        assert!(matches!(
            ctx,
            CompletionContext::Argument {
                kind: ArgCompletionKind::FilePath,
                ..
            }
        ));
    }

    #[test]
    fn context_source_is_file() {
        let ctx = resolve_completion_context("so main.vim", 11);
        assert!(matches!(
            ctx,
            CompletionContext::Argument {
                kind: ArgCompletionKind::FilePath,
                ..
            }
        ));
    }

    #[test]
    fn context_read_is_file() {
        let ctx = resolve_completion_context("r data.txt", 10);
        assert!(matches!(
            ctx,
            CompletionContext::Argument {
                kind: ArgCompletionKind::FilePath,
                ..
            }
        ));
    }

    #[test]
    fn context_tabedit_is_file() {
        let ctx = resolve_completion_context("tabe file", 9);
        assert!(matches!(
            ctx,
            CompletionContext::Argument {
                kind: ArgCompletionKind::FilePath,
                ..
            }
        ));
    }

    #[test]
    fn context_sbuffer_is_buffer() {
        let ctx = resolve_completion_context("sb main", 7);
        assert!(matches!(
            ctx,
            CompletionContext::Argument {
                kind: ArgCompletionKind::Buffer,
                ..
            }
        ));
    }

    #[test]
    fn context_setlocal_is_setting() {
        let ctx = resolve_completion_context("setl wrap", 9);
        assert!(matches!(
            ctx,
            CompletionContext::Argument {
                kind: ArgCompletionKind::Setting,
                ..
            }
        ));
    }

    #[test]
    fn context_setglobal_is_setting() {
        let ctx = resolve_completion_context("setg wrap", 9);
        assert!(matches!(
            ctx,
            CompletionContext::Argument {
                kind: ArgCompletionKind::Setting,
                ..
            }
        ));
    }

    #[test]
    fn context_cursor_mid_input() {
        // Cursor at position 2 in "edit src/" -- only "ed" visible to resolver
        let ctx = resolve_completion_context("edit src/", 2);
        assert!(matches!(ctx, CompletionContext::CommandName { ref prefix, .. } if prefix == "ed"));
    }

    #[test]
    fn context_whitespace_only() {
        let ctx = resolve_completion_context("   ", 3);
        assert!(matches!(ctx, CompletionContext::None));
    }

    // ═══════════════════════════════════════════════════════════════════
    // matches_cmd helper tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn matches_cmd_exact_min() {
        assert!(matches_cmd("e", "e", "edit"));
    }

    #[test]
    fn matches_cmd_exact_full() {
        assert!(matches_cmd("edit", "e", "edit"));
    }

    #[test]
    fn matches_cmd_mid_abbreviation() {
        assert!(matches_cmd("edi", "e", "edit"));
    }

    #[test]
    fn matches_cmd_too_short() {
        // "s" is shorter than min "se" for "set"
        assert!(!matches_cmd("s", "se", "set"));
    }

    #[test]
    fn matches_cmd_too_long() {
        assert!(!matches_cmd("edits", "e", "edit"));
    }

    #[test]
    fn matches_cmd_case_insensitive() {
        assert!(matches_cmd("EDIT", "e", "edit"));
        assert!(matches_cmd("Edit", "e", "edit"));
    }

    #[test]
    fn matches_cmd_wrong_prefix() {
        assert!(!matches_cmd("xd", "e", "edit"));
    }

    // ═══════════════════════════════════════════════════════════════════
    // OPTION_NAMES table invariants
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn option_names_table_is_sorted() {
        for pair in OPTION_NAMES.windows(2) {
            assert!(
                pair[0] < pair[1],
                "OPTION_NAMES not sorted: {:?} >= {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn commentstring_has_no_short_name() {
        // `:set` knows 'commentstring' by its full name only, so completion
        // offers no `cms`.
        let names: Vec<_> = complete_setting_name("c", &VimOptions::default())
            .into_iter()
            .map(|c| c.text)
            .collect();
        assert!(!names.iter().any(|n| n.as_str() == "cms"), "{names:?}");
        assert!(
            names.iter().any(|n| n.as_str() == "commentstring"),
            "{names:?}"
        );
    }

    #[test]
    fn option_descriptions_cover_all_canonical_names() {
        // Every canonical name that appears in OPTION_NAMES should have a
        // description entry (abbreviations map to their canonical via
        // canonical_option_name, so we only check canonical forms).
        let canonical_names: Vec<&str> = OPTION_NAMES
            .iter()
            .map(|&n| canonical_option_name(n))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        for name in &canonical_names {
            assert!(
                option_description(name).is_some(),
                "Missing description for canonical option: {name:?}"
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // complete_setting_name tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn setting_name_completion_prefix_scr() {
        let opts = VimOptions::default();
        let results = complete_setting_name("scr", &opts);
        assert!(
            results.iter().any(|c| c.text.as_str() == "scrolloff"),
            "Expected 'scrolloff' in results: {results:?}"
        );
    }

    #[test]
    fn setting_name_completion_prefix_hl() {
        let opts = VimOptions::default();
        let results = complete_setting_name("hl", &opts);
        assert!(
            results.iter().any(|c| c.text.as_str() == "hls"),
            "Expected 'hls' in results: {results:?}"
        );
        assert!(
            results.iter().any(|c| c.text.as_str() == "hlsearch"),
            "Expected 'hlsearch' in results: {results:?}"
        );
    }

    #[test]
    fn setting_name_completion_shows_current_value_bool() {
        let opts = VimOptions::default();
        // hlsearch defaults to true
        let results = complete_setting_name("hlsearch", &opts);
        let candidate = results
            .iter()
            .find(|c| c.text.as_str() == "hlsearch")
            .unwrap();
        assert_eq!(candidate.description.as_deref(), Some("[on]"));
    }

    #[test]
    fn setting_name_completion_shows_current_value_numeric() {
        let opts = VimOptions::default();
        // scrolloff defaults to 5
        let results = complete_setting_name("scrolloff", &opts);
        let candidate = results
            .iter()
            .find(|c| c.text.as_str() == "scrolloff")
            .unwrap();
        assert_eq!(candidate.description.as_deref(), Some("[= 5]"));
    }

    #[test]
    fn setting_name_completion_shows_current_value_string() {
        let opts = VimOptions::default();
        // selection defaults to "inclusive"
        let results = complete_setting_name("selection", &opts);
        let candidate = results
            .iter()
            .find(|c| c.text.as_str() == "selection")
            .unwrap();
        assert_eq!(candidate.description.as_deref(), Some("[= inclusive]"));
    }

    #[test]
    fn setting_name_completion_shows_detail() {
        let opts = VimOptions::default();
        let results = complete_setting_name("hlsearch", &opts);
        let candidate = results
            .iter()
            .find(|c| c.text.as_str() == "hlsearch")
            .unwrap();
        assert_eq!(
            candidate.detail.as_deref(),
            Some("Highlight all search matches")
        );
    }

    #[test]
    fn setting_name_completion_no_prefix_nohl() {
        let opts = VimOptions::default();
        let results = complete_setting_name("nohl", &opts);
        assert!(
            results.iter().any(|c| c.text.as_str() == "nohlsearch"),
            "Expected 'nohlsearch' in results: {results:?}"
        );
    }

    #[test]
    fn setting_name_completion_no_prefix_noig() {
        let opts = VimOptions::default();
        let results = complete_setting_name("noig", &opts);
        assert!(
            results.iter().any(|c| c.text.as_str() == "noignorecase"),
            "Expected 'noignorecase' in results: {results:?}"
        );
    }

    #[test]
    fn setting_name_completion_no_form_shows_toggle_info() {
        let opts = VimOptions::default();
        // hlsearch is ON by default, so nohlsearch should show [on -> off]
        let results = complete_setting_name("nohlsearch", &opts);
        let candidate = results
            .iter()
            .find(|c| c.text.as_str() == "nohlsearch")
            .unwrap();
        assert_eq!(candidate.description.as_deref(), Some("[on -> off]"));
    }

    #[test]
    fn setting_name_completion_empty_prefix_returns_all() {
        let opts = VimOptions::default();
        let results = complete_setting_name("", &opts);
        // Should include all option names plus no{bool} forms
        assert!(results.len() > OPTION_NAMES.len());
        assert!(results.iter().any(|c| c.text.as_str() == "scrolloff"));
        assert!(results.iter().any(|c| c.text.as_str() == "nohlsearch"));
    }

    #[test]
    fn setting_name_completion_nonexistent_returns_empty() {
        let opts = VimOptions::default();
        let results = complete_setting_name("zzzzz", &opts);
        assert!(results.is_empty());
    }

    #[test]
    fn setting_name_completion_abbreviation_ts() {
        let opts = VimOptions::default();
        let results = complete_setting_name("ts", &opts);
        assert!(
            results.iter().any(|c| c.text.as_str() == "ts"),
            "Expected 'ts' (tabstop abbrev) in results: {results:?}"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // complete_setting_value tests
    // ═══════════════════════════════════════════════════════════════════

    #[test]
    fn setting_value_completion_selection_enum() {
        let opts = VimOptions::default();
        let results = complete_setting_value("selection", "", &opts);
        assert_eq!(results.len(), 3);
        assert!(results.iter().any(|c| c.text.as_str() == "inclusive"));
        assert!(results.iter().any(|c| c.text.as_str() == "exclusive"));
        assert!(results.iter().any(|c| c.text.as_str() == "old"));
    }

    #[test]
    fn setting_value_completion_selection_marks_current() {
        let opts = VimOptions::default();
        // Default selection is "inclusive"
        let results = complete_setting_value("selection", "", &opts);
        let inclusive = results
            .iter()
            .find(|c| c.text.as_str() == "inclusive")
            .unwrap();
        assert_eq!(inclusive.description.as_deref(), Some("[current]"));
        let exclusive = results
            .iter()
            .find(|c| c.text.as_str() == "exclusive")
            .unwrap();
        assert!(exclusive.description.is_none());
    }

    #[test]
    fn setting_value_completion_selection_with_prefix() {
        let opts = VimOptions::default();
        let results = complete_setting_value("selection", "ex", &opts);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].text.as_str(), "exclusive");
    }

    #[test]
    fn setting_value_completion_inccommand_enum() {
        let opts = VimOptions::default();
        let results = complete_setting_value("inccommand", "", &opts);
        assert!(results.iter().any(|c| c.text.as_str() == "nosplit"));
        assert!(results.iter().any(|c| c.text.as_str() == "split"));
    }

    #[test]
    fn setting_value_completion_clipboard_enum() {
        let opts = VimOptions::default();
        let results = complete_setting_value("clipboard", "", &opts);
        assert!(results.iter().any(|c| c.text.as_str() == "unnamed"));
        assert!(results.iter().any(|c| c.text.as_str() == "unnamedplus"));
    }

    #[test]
    fn setting_value_completion_bool_option() {
        let opts = VimOptions::default();
        let results = complete_setting_value("hlsearch", "", &opts);
        assert!(results.iter().any(|c| c.text.as_str() == "hlsearch"));
        assert!(results.iter().any(|c| c.text.as_str() == "nohlsearch"));
    }

    #[test]
    fn setting_value_completion_numeric_returns_empty() {
        let opts = VimOptions::default();
        let results = complete_setting_value("scrolloff", "", &opts);
        assert!(results.is_empty());
    }

    #[test]
    fn setting_value_completion_unknown_returns_empty() {
        let opts = VimOptions::default();
        let results = complete_setting_value("nonexistent", "", &opts);
        assert!(results.is_empty());
    }

    #[test]
    fn setting_value_completion_abbreviation_resolves() {
        let opts = VimOptions::default();
        // "sel" is abbreviation for "selection"
        let results = complete_setting_value("sel", "", &opts);
        assert_eq!(results.len(), 3);
        assert!(results.iter().any(|c| c.text.as_str() == "inclusive"));
    }

    #[test]
    fn setting_value_completion_virtualedit_enum() {
        let opts = VimOptions::default();
        let results = complete_setting_value("virtualedit", "", &opts);
        assert!(results.iter().any(|c| c.text.as_str() == "block"));
        assert!(results.iter().any(|c| c.text.as_str() == "insert"));
        assert!(results.iter().any(|c| c.text.as_str() == "all"));
        assert!(results.iter().any(|c| c.text.as_str() == "onemore"));
    }
}
