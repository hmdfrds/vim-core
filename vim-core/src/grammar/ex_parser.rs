//! Ex command parser.
//!
//! Parses textual command-line ex input into typed [`crate::grammar::types::ExCommand`].
//! This layer is grammar-only: no execution or shell dependencies.

use crate::errors::VimError;
use crate::grammar::types::{
    ExCommand, ExRange, LineSpec, MapModePrefix, ModifierFlags, RangeSeparator, SetAssignment,
    SortOptions, TimeAmount,
};
use crate::primitives::{AbbrevMode, MarkName, SubFlags};
use compact_str::CompactString;

use super::command_meta;

/// Parse an ex command line into a typed command (discarding any modifier prefixes).
///
/// # Errors
///
/// Returns a typed `VimError` for invalid syntax.
pub fn parse_ex_command(input: &str) -> Result<ExCommand, VimError> {
    parse_ex_command_with_modifiers(input).map(|(_, cmd)| cmd)
}

/// Parse an ex command line, returning both modifier flags and the command.
///
/// Modifier prefixes (`:silent`, `:keepjumps`, etc.) are stripped from the
/// front and collected into [`ModifierFlags`]. Multiple modifiers can be
/// chained: `:silent keepjumps d3j`.
///
/// # Errors
///
/// Returns a typed `VimError` for invalid syntax.
pub fn parse_ex_command_with_modifiers(
    input: &str,
) -> Result<(ModifierFlags, ExCommand), VimError> {
    let (modifiers, rest) = parse_modifier_prefixes(input);
    let cmd = parse_ex_command_inner(rest)?;
    Ok((modifiers, cmd))
}

/// Strip modifier keyword prefixes from the input, returning accumulated
/// flags and the remaining unparsed input.
fn parse_modifier_prefixes(input: &str) -> (ModifierFlags, &str) {
    let mut flags = ModifierFlags::empty();
    let mut rest = input;

    loop {
        let trimmed = rest.trim_start();
        // Try to match a modifier keyword at the start. Modifiers must be
        // followed by whitespace or end-of-input to avoid misparsing
        // commands that happen to start with the same prefix (e.g.,
        // `:silently` is not `:silent` + `ly`).
        if let Some(after) = strip_modifier_prefix(trimmed, "silent") {
            // `:silent!` — check for bang immediately after "silent"
            if let Some(after_bang) = after.strip_prefix('!') {
                flags |= ModifierFlags::SILENT | ModifierFlags::SILENT_BANG;
                rest = after_bang;
            } else {
                flags |= ModifierFlags::SILENT;
                rest = after;
            }
        } else if let Some(after) = strip_modifier_prefix(trimmed, "keepjumps") {
            flags |= ModifierFlags::KEEPJUMPS;
            rest = after;
        } else if let Some(after) = strip_modifier_prefix(trimmed, "keeppatterns") {
            flags |= ModifierFlags::KEEPPATTERNS;
            rest = after;
        } else if let Some(after) = strip_modifier_prefix(trimmed, "lockmarks") {
            flags |= ModifierFlags::LOCKMARKS;
            rest = after;
        } else if let Some(after) = strip_modifier_prefix(trimmed, "keepalt") {
            flags |= ModifierFlags::KEEPALT;
            rest = after;
        } else if let Some(after) = strip_modifier_prefix(trimmed, "tab") {
            flags |= ModifierFlags::TAB;
            rest = after;
        } else if let Some(after) = strip_modifier_prefix(trimmed, "vertical") {
            flags |= ModifierFlags::VERTICAL;
            rest = after;
        } else if let Some(after) = strip_modifier_prefix(trimmed, "vert") {
            flags |= ModifierFlags::VERTICAL;
            rest = after;
        } else if let Some(after) = strip_modifier_prefix(trimmed, "horizontal") {
            flags |= ModifierFlags::HORIZONTAL;
            rest = after;
        } else if let Some(after) = strip_modifier_prefix(trimmed, "hor") {
            flags |= ModifierFlags::HORIZONTAL;
            rest = after;
        } else if let Some(after) = strip_modifier_prefix(trimmed, "topleft") {
            flags |= ModifierFlags::TOPLEFT;
            rest = after;
        } else if let Some(after) = strip_modifier_prefix(trimmed, "botright") {
            flags |= ModifierFlags::BOTRIGHT;
            rest = after;
        } else if let Some(after) = strip_modifier_prefix(trimmed, "aboveleft") {
            flags |= ModifierFlags::ABOVELEFT;
            rest = after;
        } else if let Some(after) = strip_modifier_prefix(trimmed, "belowright") {
            flags |= ModifierFlags::BELOWRIGHT;
            rest = after;
        } else if let Some(after) = strip_modifier_prefix(trimmed, "browse") {
            flags |= ModifierFlags::BROWSE;
            rest = after;
        } else if let Some(after) = strip_modifier_prefix(trimmed, "confirm") {
            flags |= ModifierFlags::CONFIRM;
            rest = after;
        } else {
            break;
        }
    }

    (flags, rest)
}

/// Try to strip a modifier keyword from the start of input.
///
/// Returns `Some(remaining)` if `input` starts with `keyword` followed by
/// whitespace (or end-of-input). The returned slice starts at the first
/// non-keyword character (the whitespace or empty tail).
fn strip_modifier_prefix<'a>(input: &'a str, keyword: &str) -> Option<&'a str> {
    let stripped = input.strip_prefix(keyword)?;
    // Must be followed by whitespace, '!', or end-of-input to be a real modifier.
    // This prevents `:silently` from matching as `:silent` + `ly`.
    match stripped.as_bytes().first() {
        None => Some(stripped),               // modifier with no command after
        Some(b' ' | b'\t') => Some(stripped), // normal separator
        Some(b'!') if keyword == "silent" => Some(stripped), // `:silent!`
        _ => None,                            // part of a longer word
    }
}

fn parse_ex_command_inner(input: &str) -> Result<ExCommand, VimError> {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        // Empty or whitespace-only command is a no-op in Vim (no error).
        // Return GotoLine with current line range so the executor treats it
        // as a harmless cursor repositioning.
        return Ok(ExCommand::GotoLine {
            range: ExRange::current_line(),
        });
    }

    let (range, rest) = parse_optional_range(trimmed)?;
    let rest = rest.trim_start();
    if rest.is_empty() {
        // Bare line address (`:3`, `:$`, etc.) — go to that line
        if let Some(range) = range {
            return Ok(ExCommand::GotoLine { range });
        }
        return Err(VimError::NotEditorCommand(CompactString::new("")));
    }

    if let Some(cmd) = rest.strip_prefix('!') {
        let command = cmd.trim();
        if command.is_empty() {
            return Err(VimError::NoPreviousCommand);
        }
        let command = CompactString::from(command);
        return Ok(if let Some(range) = range {
            ExCommand::Filter { range, command }
        } else {
            ExCommand::External { command }
        });
    }

    // Repeat-substitute commands: `:&` and `:&&`.
    // Must be checked before the named-command path since `&` is not alphabetic.
    if rest.starts_with("&&") {
        // `:&&` — repeat last substitute keeping same flags.
        return Ok(ExCommand::RepeatSubstitute {
            range,
            use_previous_flags: true,
        });
    }
    if rest.starts_with('&') {
        // `:&` — repeat last substitute with no flags.
        return Ok(ExCommand::RepeatSubstitute {
            range,
            use_previous_flags: false,
        });
    }

    // `:~` — substitute using last search pattern + last substitute replacement.
    if let Some(flags_str) = rest.strip_prefix('~') {
        return Ok(ExCommand::SubTilde {
            range: range.unwrap_or_else(ExRange::current_line),
            flags: SubFlags::parse(flags_str.trim()),
        });
    }

    // Substitute/global commands: only match when followed by a delimiter
    // (non-alphanumeric, non-underscore). This prevents `:set`, `:sort`,
    // `:goto`, `:verbose` etc. from being misparsed.
    if rest.starts_with('s') && is_command_delimiter(rest.as_bytes().get(1).copied()) {
        return parse_substitute(rest, range.unwrap_or_else(ExRange::current_line));
    }
    // `:g!` — global inversion alias (same as `:v`). Must come before bare `:g`.
    if rest.starts_with("g!") && is_command_delimiter(rest.as_bytes().get(2).copied()) {
        // Skip "g!" so input starts at the delimiter, matching what parse_global_from_delim expects.
        return parse_global_from_delim(
            &rest[2..],
            true,
            range.unwrap_or_else(ExRange::entire_file),
        );
    }
    if rest.starts_with('g') && is_command_delimiter(rest.as_bytes().get(1).copied()) {
        return parse_global(rest, false, range.unwrap_or_else(ExRange::entire_file));
    }
    if rest.starts_with('v') && is_command_delimiter(rest.as_bytes().get(1).copied()) {
        return parse_global(rest, true, range.unwrap_or_else(ExRange::entire_file));
    }

    // Structural regex commands: :sx/pattern/command and :sy/pattern/command
    {
        if rest.starts_with("sx") && is_command_delimiter(rest.as_bytes().get(2).copied()) {
            return parse_structural(rest, false);
        }
        if rest.starts_with("sy") && is_command_delimiter(rest.as_bytes().get(2).copied()) {
            return parse_structural(rest, true);
        }
    }

    // Multi-cursor selection ex commands: :select/:split/:keep/:remove/:trim/:align/:rotate
    if let Some(cmd) = parse_multi_cursor_ex(rest, range.clone())? {
        return Ok(cmd);
    }

    // `:@{register}` — execute register contents as ex commands.
    // Must be checked before `parse_named_command` since `@` followed by a
    // non-alpha char (e.g., `@0`, `@"`) trips up `split_head`.
    if let Some(reg_suffix) = rest.strip_prefix('@') {
        let mut chars = reg_suffix.chars();
        if let Some(ch) = chars.next() {
            // Only accept a single-char register name immediately after `@`.
            if chars.next().is_none() {
                if let Some(register) = crate::primitives::RegisterName::new(ch) {
                    return Ok(ExCommand::ExecuteRegister { register });
                }
            }
        }
    }

    parse_named_command(rest, range)
}

/// Check if a byte is a valid delimiter for s/g/v commands.
///
/// Returns true for non-alphanumeric, non-underscore characters (or None = end of input).
/// This prevents `:set` from matching as `:s` + `et`, etc.
#[inline]
const fn is_command_delimiter(byte: Option<u8>) -> bool {
    match byte {
        None => false, // bare `s`/`g`/`v` alone — route to named command
        Some(b) => !b.is_ascii_alphanumeric() && b != b'_',
    }
}

/// Check if `name` matches the abbreviation range from `min` to `full` (case-insensitive).
///
/// Implements Vim's bracket abbreviation notation: `d[elete]` means `min="d"`, `full="delete"`.
/// Any prefix of `full` that is at least `min.len()` characters long matches.
/// For example, `d`, `de`, `del`, `dele`, `delet`, `delete` all match `d[elete]`.
#[inline]
fn matches_abbrev(name: &str, min: &str, full: &str) -> bool {
    let n = name.len();
    n >= min.len() && n <= full.len() && name.eq_ignore_ascii_case(&full[..n])
}

fn parse_named_command(rest: &str, range: Option<ExRange>) -> Result<ExCommand, VimError> {
    let (head, tail) = split_head(rest);
    let force = head.ends_with('!');
    let name = head.trim_end_matches('!');

    // Strip trailing comments for commands that allow them.
    // Commands with TRLBAR (but not NOTRLCOM) treat `" ...` as a comment.
    let tail = if let Some(meta) = command_meta::meta_for_command(name) {
        if meta.allows_trailing_comment() {
            command_meta::strip_trailing_comment(tail)
        } else {
            tail
        }
    } else {
        tail
    };

    // Pre-compute read's after_line before consuming range (borrows only).
    let read_after_line = read_after_line(range.as_ref());
    let retab_range = range.clone().unwrap_or_else(ExRange::entire_file);
    let line_range = range.unwrap_or_else(ExRange::current_line);

    // Abbreviation-style matching: matches_abbrev(name, min, full) accepts any
    // prefix of `full` that is >= `min` characters. This implements Vim's
    // `min[rest]` bracket notation (e.g. `d[elete]`, `noh[lsearch]`).
    //
    // Ordering matters: more specific prefixes must come before more general
    // ones to prevent ambiguity (e.g. `seth[andler]` before `se[t]`).

    if matches_abbrev(name, "d", "delete") {
        Ok(ExCommand::Delete {
            range: line_range,
            register: parse_optional_register(tail),
        })
    } else if matches_abbrev(name, "y", "yank") {
        Ok(ExCommand::Yank {
            range: line_range,
            register: parse_optional_register(tail),
        })
    } else if matches_abbrev(name, "m", "move") {
        Ok(ExCommand::Move {
            range: line_range,
            target: parse_line_spec_exact(tail)?,
        })
    } else if name.eq_ignore_ascii_case("t") || matches_abbrev(name, "co", "copy") {
        Ok(ExCommand::Copy {
            range: line_range,
            target: parse_line_spec_exact(tail)?,
        })
    } else if matches_abbrev(name, "j", "join") {
        Ok(ExCommand::Join {
            range: line_range,
            bang: force,
        })
    } else if matches_abbrev(name, "sor", "sort") {
        // :sort! means reverse — the bang is consumed by force detection,
        // so pass it through to SortOptions
        let options = if force {
            SortOptions::parse_reversed(tail)
        } else {
            SortOptions::parse(tail)
        };
        Ok(ExCommand::Sort {
            range: line_range,
            options,
        })
    } else if matches_abbrev(name, "pu", "put") {
        Ok(ExCommand::Put {
            range: line_range,
            register: parse_optional_register(tail),
            before: force,
        })
    } else if matches_abbrev(name, "ret", "retab") {
        Ok(ExCommand::Retab {
            range: retab_range,
            new_tabstop: tail.trim().parse::<usize>().ok(),
            to_tabs: force,
        })
    } else if matches_abbrev(name, "le", "left") {
        Ok(ExCommand::Left {
            range: line_range,
            indent: tail.trim().parse::<usize>().unwrap_or(0),
        })
    } else if matches_abbrev(name, "ri", "right") {
        Ok(ExCommand::Right {
            range: line_range,
            width: tail.trim().parse::<usize>().ok(),
        })
    } else if matches_abbrev(name, "ce", "center") || matches_abbrev(name, "ce", "centre") {
        Ok(ExCommand::Center {
            range: line_range,
            width: tail.trim().parse::<usize>().ok(),
        })
    } else if matches_abbrev(name, "noh", "nohlsearch") {
        Ok(ExCommand::NoHighlight)
    } else if matches_abbrev(name, "reg", "registers") || matches_abbrev(name, "di", "display") {
        Ok(ExCommand::Registers {
            filter: if tail.is_empty() {
                None
            } else {
                Some(CompactString::from(tail))
            },
        })
    } else if matches_abbrev(name, "mar", "marks") {
        Ok(ExCommand::Marks {
            filter: if tail.is_empty() {
                None
            } else {
                Some(CompactString::from(tail))
            },
        })
    } else if matches_abbrev(name, "ju", "jumps") {
        Ok(ExCommand::Jumps)
    } else if matches_abbrev(name, "cha", "changes") {
        Ok(ExCommand::Changes)
    } else if matches_abbrev(name, "clearj", "clearjumps") {
        Ok(ExCommand::ClearJumps)
    } else if matches_abbrev(name, "mes", "messages") {
        let clear = tail.trim().eq_ignore_ascii_case("clear");
        Ok(ExCommand::Messages { clear })
    // Abbreviation commands — must come before mapping commands to avoid
    // prefix conflicts (e.g. "norea" before "no"/"noremap", "inorea" before
    // "ino"/"inoremap", "cnorea" before "cno"/"cnoremap", "iuna" before
    // "iu"/"iunmap", "cuna" before "cu"/"cunmap").
    } else if matches_abbrev(name, "ab", "abbreviate") {
        parse_abbreviate_command(tail, AbbrevMode::Both, false)
    } else if matches_abbrev(name, "ia", "iabbrev") {
        parse_abbreviate_command(tail, AbbrevMode::Insert, false)
    } else if matches_abbrev(name, "ca", "cabbrev") {
        parse_abbreviate_command(tail, AbbrevMode::CommandLine, false)
    } else if matches_abbrev(name, "norea", "noreabbrev") {
        parse_abbreviate_command(tail, AbbrevMode::Both, true)
    } else if matches_abbrev(name, "inorea", "inoreabbrev") {
        parse_abbreviate_command(tail, AbbrevMode::Insert, true)
    } else if matches_abbrev(name, "cnorea", "cnoreabbrev") {
        parse_abbreviate_command(tail, AbbrevMode::CommandLine, true)
    } else if matches_abbrev(name, "una", "unabbreviate") {
        parse_unabbreviate_command(tail, AbbrevMode::Both)
    } else if matches_abbrev(name, "iuna", "iunabbrev") {
        parse_unabbreviate_command(tail, AbbrevMode::Insert)
    } else if matches_abbrev(name, "cuna", "cunabbrev") {
        parse_unabbreviate_command(tail, AbbrevMode::CommandLine)
    } else if matches_abbrev(name, "abc", "abclear") {
        Ok(ExCommand::AbClear {
            mode: AbbrevMode::Both,
        })
    } else if matches_abbrev(name, "iabc", "iabclear") {
        Ok(ExCommand::AbClear {
            mode: AbbrevMode::Insert,
        })
    } else if matches_abbrev(name, "cabc", "cabclear") {
        Ok(ExCommand::AbClear {
            mode: AbbrevMode::CommandLine,
        })
    // Mapping commands — recursive
    } else if name.eq_ignore_ascii_case("map") {
        parse_map_command(tail, MapModePrefix::All, false)
    } else if matches_abbrev(name, "nm", "nmap") {
        parse_map_command(tail, MapModePrefix::Normal, false)
    } else if matches_abbrev(name, "vm", "vmap") {
        parse_map_command(tail, MapModePrefix::Visual, false)
    } else if matches_abbrev(name, "im", "imap") {
        parse_map_command(tail, MapModePrefix::Insert, false)
    } else if matches_abbrev(name, "om", "omap") {
        parse_map_command(tail, MapModePrefix::Operator, false)
    } else if matches_abbrev(name, "cm", "cmap") {
        parse_map_command(tail, MapModePrefix::Command, false)
    // Mapping commands — non-recursive
    } else if matches_abbrev(name, "no", "noremap") {
        parse_map_command(tail, MapModePrefix::All, true)
    } else if matches_abbrev(name, "nn", "nnoremap") {
        parse_map_command(tail, MapModePrefix::Normal, true)
    } else if matches_abbrev(name, "vn", "vnoremap") {
        parse_map_command(tail, MapModePrefix::Visual, true)
    } else if matches_abbrev(name, "ino", "inoremap") {
        parse_map_command(tail, MapModePrefix::Insert, true)
    } else if matches_abbrev(name, "ono", "onoremap") {
        parse_map_command(tail, MapModePrefix::Operator, true)
    } else if matches_abbrev(name, "cno", "cnoremap") {
        parse_map_command(tail, MapModePrefix::Command, true)
    // Unmap commands
    } else if matches_abbrev(name, "unm", "unmap") {
        parse_unmap_command(tail, MapModePrefix::All)
    } else if matches_abbrev(name, "nun", "nunmap") {
        parse_unmap_command(tail, MapModePrefix::Normal)
    } else if matches_abbrev(name, "vu", "vunmap") {
        parse_unmap_command(tail, MapModePrefix::Visual)
    } else if matches_abbrev(name, "iu", "iunmap") {
        parse_unmap_command(tail, MapModePrefix::Insert)
    } else if matches_abbrev(name, "ou", "ounmap") {
        parse_unmap_command(tail, MapModePrefix::Operator)
    } else if matches_abbrev(name, "cu", "cunmap") {
        parse_unmap_command(tail, MapModePrefix::Command)
    // xmap/xnoremap/xunmap (visual-only)
    } else if matches_abbrev(name, "xm", "xmap") {
        parse_map_command(tail, MapModePrefix::VisualOnly, false)
    } else if matches_abbrev(name, "xn", "xnoremap") {
        parse_map_command(tail, MapModePrefix::VisualOnly, true)
    } else if matches_abbrev(name, "xu", "xunmap") {
        parse_unmap_command(tail, MapModePrefix::VisualOnly)
    // smap/snoremap/sunmap (select-only)
    } else if matches_abbrev(name, "sn", "snoremap") {
        parse_map_command(tail, MapModePrefix::SelectOnly, true)
    } else if matches_abbrev(name, "su", "sunmap") {
        parse_unmap_command(tail, MapModePrefix::SelectOnly)
    } else if matches_abbrev(name, "sm", "smap") {
        parse_map_command(tail, MapModePrefix::SelectOnly, false)
    // mapclear commands
    } else if name.eq_ignore_ascii_case("mapclear") {
        Ok(ExCommand::MapClear {
            mode: MapModePrefix::All,
            force,
        })
    } else if matches_abbrev(name, "nmapc", "nmapclear") {
        Ok(ExCommand::MapClear {
            mode: MapModePrefix::Normal,
            force,
        })
    } else if matches_abbrev(name, "vmapc", "vmapclear") {
        Ok(ExCommand::MapClear {
            mode: MapModePrefix::Visual,
            force,
        })
    } else if matches_abbrev(name, "imapc", "imapclear") {
        Ok(ExCommand::MapClear {
            mode: MapModePrefix::Insert,
            force,
        })
    } else if matches_abbrev(name, "omapc", "omapclear") {
        Ok(ExCommand::MapClear {
            mode: MapModePrefix::Operator,
            force,
        })
    } else if matches_abbrev(name, "cmapc", "cmapclear") {
        Ok(ExCommand::MapClear {
            mode: MapModePrefix::Command,
            force,
        })
    } else if matches_abbrev(name, "xmapc", "xmapclear") {
        Ok(ExCommand::MapClear {
            mode: MapModePrefix::VisualOnly,
            force,
        })
    } else if matches_abbrev(name, "smapc", "smapclear") {
        Ok(ExCommand::MapClear {
            mode: MapModePrefix::SelectOnly,
            force,
        })
    // Options — more specific prefixes must come before general ones.
    // sethandler (seth) > setlocal (setl) > setglobal (setg) > set (se).
    } else if matches_abbrev(name, "seth", "sethandler") {
        parse_sethandler(tail)
    } else if matches_abbrev(name, "setl", "setlocal") {
        Ok(ExCommand::SetLocal {
            assignments: parse_set_assignments(tail),
        })
    } else if matches_abbrev(name, "setg", "setglobal") {
        Ok(ExCommand::SetGlobal {
            assignments: parse_set_assignments(tail),
        })
    } else if matches_abbrev(name, "se", "set") {
        Ok(ExCommand::Set {
            assignments: parse_set_assignments(tail),
        })
    } else if matches_abbrev(name, "w", "write") {
        Ok(ExCommand::Write {
            path: if tail.is_empty() {
                None
            } else {
                Some(CompactString::from(tail))
            },
            force,
        })
    } else if matches_abbrev(name, "q", "quit") {
        Ok(ExCommand::Quit { force })
    } else if name.eq_ignore_ascii_case("wq") || matches_abbrev(name, "x", "xit") {
        Ok(ExCommand::WriteQuit { force })
    } else if matches_abbrev(name, "e", "edit") {
        parse_required_path_command(tail, |path| ExCommand::Edit { path, force })
    } else if matches_abbrev(name, "r", "read") {
        parse_required_path_command(tail, |path| ExCommand::Read {
            path,
            after_line: read_after_line,
        })
    } else if matches_abbrev(name, "norm", "normal") {
        let (_, raw_tail) = split_head_raw(rest);
        let remap = !force;
        if raw_tail.is_empty() {
            return Err(VimError::ArgumentRequired);
        }
        Ok(ExCommand::Norm {
            range: line_range,
            keys: CompactString::from(raw_tail),
            remap,
        })
    } else if name.eq_ignore_ascii_case("action") {
        parse_required_path_command(tail, |name| ExCommand::Action { name })
    } else if matches_abbrev(name, "actionl", "actionlist") {
        Ok(ExCommand::ActionList {
            filter: if tail.is_empty() {
                None
            } else {
                Some(CompactString::from(tail))
            },
        })
    } else if matches_abbrev(name, "so", "source") {
        parse_required_path_command(tail, |path| ExCommand::Source { path })
    } else if matches_abbrev(name, "ea", "earlier") {
        parse_time_amount(tail).map(|amount| ExCommand::Earlier { amount })
    } else if matches_abbrev(name, "lat", "later") {
        parse_time_amount(tail).map(|amount| ExCommand::Later { amount })
    } else if matches_abbrev(name, "undol", "undolist") {
        Ok(ExCommand::UndoList)
    } else if matches_abbrev(name, "undot", "undotree") {
        Ok(ExCommand::UndoTree)
    } else if matches_abbrev(name, "red", "redo") {
        Ok(ExCommand::Redo)
    } else if name.eq_ignore_ascii_case("undo") || name.eq_ignore_ascii_case("u") {
        let trimmed = tail.trim();
        if trimmed.is_empty() {
            // Plain `:undo` with no argument = undo one change (go to parent).
            // Treat as `:earlier 1` for simplicity.
            Ok(ExCommand::Earlier {
                amount: TimeAmount::Changes(1),
            })
        } else {
            let seq: u64 = trimmed
                .parse()
                .map_err(|_| VimError::InvalidArgument(CompactString::from(trimmed)))?;
            Ok(ExCommand::UndoSequence { seq })
        }
    // Buffer navigation
    } else if matches_abbrev(name, "b", "buffer") {
        let n = parse_count_arg(tail)?;
        Ok(ExCommand::Buffer { number: n })
    } else if matches_abbrev(name, "bn", "bnext") {
        Ok(ExCommand::BufferNext {
            count: parse_count_or_default(tail),
        })
    } else if matches_abbrev(name, "bp", "bprevious") {
        Ok(ExCommand::BufferPrev {
            count: parse_count_or_default(tail),
        })
    } else if matches_abbrev(name, "bf", "bfirst") || matches_abbrev(name, "bre", "brewind") {
        Ok(ExCommand::BufferFirst)
    } else if matches_abbrev(name, "bl", "blast") {
        Ok(ExCommand::BufferLast)
    } else if name.eq_ignore_ascii_case("ls")
        || matches_abbrev(name, "buffers", "buffers")
        || matches_abbrev(name, "files", "files")
    {
        Ok(ExCommand::BufferList)
    // Tab navigation
    } else if name.eq_ignore_ascii_case("tabnew") || matches_abbrev(name, "tabe", "tabedit") {
        Ok(ExCommand::TabNew {
            path: if tail.is_empty() {
                None
            } else {
                Some(CompactString::from(tail))
            },
        })
    } else if matches_abbrev(name, "tabn", "tabnext") {
        Ok(ExCommand::TabNext {
            count: parse_count_or_default(tail),
        })
    } else if matches_abbrev(name, "tabp", "tabprevious") {
        Ok(ExCommand::TabPrev {
            count: parse_count_or_default(tail),
        })
    } else if matches_abbrev(name, "tabc", "tabclose") {
        Ok(ExCommand::TabClose { force })
    // Display / Misc
    } else if matches_abbrev(name, "ec", "echo") {
        Ok(ExCommand::Echo {
            message: CompactString::from(tail),
        })
    } else if name.eq_ignore_ascii_case("let") {
        parse_let_command(tail)
    } else if matches_abbrev(name, "p", "print") {
        Ok(ExCommand::PrintLines {
            range: line_range,
            number: false,
            list: false,
        })
    } else if matches_abbrev(name, "nu", "number") || name == "#" {
        Ok(ExCommand::PrintLines {
            range: line_range,
            number: true,
            list: false,
        })
    } else if matches_abbrev(name, "l", "list") {
        Ok(ExCommand::PrintLines {
            range: line_range,
            number: false,
            list: true,
        })
    } else if name == "z"
        || (name.len() == 2 && name.starts_with('z') && b"+-.=^#".contains(&name.as_bytes()[1]))
    {
        // Style modifier may be attached to the name (e.g., "z." from split_head).
        let style_char = if name.len() == 2 {
            Some(name.as_bytes()[1])
        } else {
            None
        };
        parse_z_command(style_char, tail, line_range)
    // Window / session / buffer management
    } else if matches_abbrev(name, "sp", "split") {
        Ok(ExCommand::Split {
            path: if tail.is_empty() {
                None
            } else {
                Some(CompactString::from(tail))
            },
        })
    } else if matches_abbrev(name, "vs", "vsplit") {
        Ok(ExCommand::VSplit {
            path: if tail.is_empty() {
                None
            } else {
                Some(CompactString::from(tail))
            },
        })
    // Diagnostic navigation — must come before clo[se] to avoid prefix clash.
    } else if matches_abbrev(name, "cn", "cnext") {
        Ok(ExCommand::CNext {
            count: parse_count_or_default(tail),
        })
    } else if matches_abbrev(name, "cp", "cprevious") || matches_abbrev(name, "cp", "cprev") {
        Ok(ExCommand::CPrev {
            count: parse_count_or_default(tail),
        })
    } else if matches_abbrev(name, "cl", "clist") {
        Ok(ExCommand::CList)
    } else if name.eq_ignore_ascii_case("cc") {
        let trimmed = tail.trim();
        Ok(ExCommand::CC {
            index: if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.parse::<u32>().unwrap_or(1))
            },
        })
    } else if matches_abbrev(name, "clo", "close") {
        Ok(ExCommand::Close { force })
    } else if name.eq_ignore_ascii_case("new") {
        Ok(ExCommand::New)
    } else if matches_abbrev(name, "vne", "vnew") {
        Ok(ExCommand::VNew)
    } else if matches_abbrev(name, "on", "only") {
        Ok(ExCommand::Only { force })
    } else if matches_abbrev(name, "wa", "wall") {
        Ok(ExCommand::WriteAll)
    } else if matches_abbrev(name, "qa", "qall") {
        Ok(ExCommand::QuitAll { force })
    } else if matches_abbrev(name, "wqa", "wqall") || matches_abbrev(name, "xa", "xall") {
        Ok(ExCommand::WriteQuitAll)
    } else if matches_abbrev(name, "bd", "bdelete") {
        Ok(ExCommand::BufferDelete {
            force,
            target: if tail.is_empty() {
                None
            } else {
                Some(CompactString::from(tail.trim()))
            },
        })
    } else if matches_abbrev(name, "bw", "bwipeout") {
        Ok(ExCommand::BufferWipeout {
            force,
            target: if tail.is_empty() {
                None
            } else {
                Some(CompactString::from(tail.trim()))
            },
        })
    // ── Delete marks ──────────────────────────────────────────────────────
    } else if matches_abbrev(name, "delm", "delmarks") {
        Ok(ExCommand::DelMarks {
            marks: CompactString::from(tail.trim()),
            clear_all: force,
        })
    // ── Quit with error code ──────────────────────────────────────────────
    } else if matches_abbrev(name, "cq", "cquit") {
        let exit_code = if tail.trim().is_empty() {
            1
        } else {
            tail.trim().parse::<i32>().unwrap_or(1)
        };
        Ok(ExCommand::CQuit { exit_code })
    // ── Update (write if modified) ────────────────────────────────────────
    } else if matches_abbrev(name, "up", "update") {
        Ok(ExCommand::Update {
            path: if tail.is_empty() {
                None
            } else {
                Some(CompactString::from(tail))
            },
            force,
        })
    // ── Fold commands ─────────────────────────────────────────────────────
    } else if matches_abbrev(name, "fo", "fold") {
        Ok(ExCommand::Fold { range: line_range })
    } else if matches_abbrev(name, "foldo", "foldopen") {
        Ok(ExCommand::FoldOpen {
            range: line_range,
            recursive: force,
        })
    } else if matches_abbrev(name, "foldc", "foldclose") {
        Ok(ExCommand::FoldClose {
            range: line_range,
            recursive: force,
        })
    // ── Iterator commands (:windo, :bufdo, :tabdo) ───────────────────
    } else if matches_abbrev(name, "windo", "windo") {
        let nested = tail.trim();
        if nested.is_empty() {
            return Err(VimError::ArgumentRequired);
        }
        let command = parse_ex_command(nested)?;
        Ok(ExCommand::WinDo {
            command: Box::new(command),
            source: compact_str::CompactString::from(nested),
        })
    } else if matches_abbrev(name, "bufdo", "bufdo") {
        let nested = tail.trim();
        if nested.is_empty() {
            return Err(VimError::ArgumentRequired);
        }
        let command = parse_ex_command(nested)?;
        Ok(ExCommand::BufDo {
            command: Box::new(command),
            source: compact_str::CompactString::from(nested),
        })
    } else if matches_abbrev(name, "tabdo", "tabdo") {
        let nested = tail.trim();
        if nested.is_empty() {
            return Err(VimError::ArgumentRequired);
        }
        let command = parse_ex_command(nested)?;
        Ok(ExCommand::TabDo {
            command: Box::new(command),
            source: compact_str::CompactString::from(nested),
        })
    // ── :undojoin ────────────────────────────────────────────────────
    } else if matches_abbrev(name, "undoj", "undojoin") {
        Ok(ExCommand::UndoJoin)
    // ── :mkvimrc ────────────────────────────────────────────────────
    } else if matches_abbrev(name, "mkv", "mkvimrc") {
        Ok(ExCommand::MkVimrc { force })
    } else {
        Ok(ExCommand::Custom {
            command: CompactString::from(rest),
        })
    }
}

fn read_after_line(range: Option<&ExRange>) -> Option<u32> {
    // Keep parser grammar-only: preserve explicit absolute addresses and defer
    // non-absolute specs (., $, marks, search, relative) to host defaults.
    let spec = match range {
        Some(r) => r.end.as_ref().unwrap_or(&r.start),
        None => return None,
    };
    match spec {
        LineSpec::Absolute(line) => Some(*line),
        _ => None,
    }
}

fn parse_required_path_command<F>(tail: &str, builder: F) -> Result<ExCommand, VimError>
where
    F: FnOnce(CompactString) -> ExCommand,
{
    if tail.is_empty() {
        return Err(VimError::ArgumentRequired);
    }
    Ok(builder(CompactString::from(tail)))
}

/// Parse a time amount for `:earlier` / `:later`.
///
/// Formats: `N` (changes), `Ns` (seconds), `Nm` (minutes), `Nh` (hours).
/// Default (no argument) is 1 change.
fn parse_time_amount(tail: &str) -> Result<TimeAmount, VimError> {
    let tail = tail.trim();
    if tail.is_empty() {
        return Ok(TimeAmount::Changes(1));
    }

    // Check for time suffix — `tail` is non-empty (checked above).
    let Some(&last) = tail.as_bytes().last() else {
        return Ok(TimeAmount::Changes(1));
    };
    match last {
        b's' | b'm' | b'h' | b'f' => {
            let num_str = &tail[..tail.len() - 1];
            if last == b'f' {
                let n: u32 = num_str
                    .parse()
                    .map_err(|_| VimError::InvalidArgument(CompactString::from(tail)))?;
                return Ok(TimeAmount::FileSaves(n));
            }
            let n: u64 = num_str
                .parse()
                .map_err(|_| VimError::InvalidArgument(CompactString::from(tail)))?;
            // Outer match guarantees last is s/m/h.
            Ok(match last {
                b's' => TimeAmount::Seconds(n),
                b'm' => TimeAmount::Minutes(n),
                // b'h' (only remaining option from the outer arm)
                _ => TimeAmount::Hours(n),
            })
        }
        _ => {
            // Pure number = change count
            let n: u32 = tail
                .parse()
                .map_err(|_| VimError::InvalidArgument(CompactString::from(tail)))?;
            Ok(TimeAmount::Changes(n))
        }
    }
}

/// Parse a required numeric argument (e.g. `:buffer 3`).
fn parse_count_arg(tail: &str) -> Result<u32, VimError> {
    let trimmed = tail.trim();
    if trimmed.is_empty() {
        return Err(VimError::ArgumentRequired);
    }
    trimmed
        .parse::<u32>()
        .map_err(|_| VimError::InvalidArgument(CompactString::from(trimmed)))
}

/// Parse an optional count argument, defaulting to 1.
fn parse_count_or_default(tail: &str) -> u32 {
    let trimmed = tail.trim();
    if trimmed.is_empty() {
        return 1;
    }
    trimmed.parse::<u32>().unwrap_or(1)
}

/// Parse `:let mapleader = "X"` (only supports mapleader assignment).
fn parse_let_command(tail: &str) -> Result<ExCommand, VimError> {
    let tail = tail.trim();

    // Match: mapleader = "X" or mapleader = 'X' or mapleader = X
    if let Some(rest) = tail
        .strip_prefix("mapleader")
        .or_else(|| tail.strip_prefix("g:mapleader"))
    {
        let rest = rest.trim();
        let Some(rest) = rest.strip_prefix('=') else {
            return Err(VimError::InvalidArgument(CompactString::from(tail)));
        };
        let rest = rest.trim();

        // Strip optional quotes
        let value = rest
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| rest.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(rest);

        let mut chars = value.chars();
        match (chars.next(), chars.next()) {
            (Some(ch), None) => Ok(ExCommand::LetMapleader { leader: ch }),
            _ => Err(VimError::InvalidArgument(CompactString::from(
                "mapleader must be a single character",
            ))),
        }
    } else {
        // Unsupported :let variant — forward as custom
        let mut cmd = CompactString::from("let ");
        cmd.push_str(tail);
        Ok(ExCommand::Custom { command: cmd })
    }
}

fn split_head(input: &str) -> (&str, &str) {
    let trimmed = input.trim_start();
    // First try splitting on whitespace
    if let Some((idx, _)) = trimmed.char_indices().find(|(_, c)| c.is_whitespace()) {
        let (head, tail) = trimmed.split_at(idx);
        (head, tail.trim())
    } else {
        // No whitespace found — try splitting at alpha/non-alpha boundary
        // This handles Vim's compact syntax like "co3" → ("co", "3"), "m$" → ("m", "$")
        if let Some((idx, _)) = trimmed
            .char_indices()
            .find(|(i, c)| *i > 0 && !c.is_alphabetic() && (*c != '!'))
        {
            let (head, tail) = trimmed.split_at(idx);
            (head, tail.trim())
        } else {
            (trimmed, "")
        }
    }
}

/// Like `split_head` but preserves trailing whitespace in the tail.
///
/// Used by commands where the tail is raw content and trailing spaces are
/// significant (e.g., `:norm` keystroke arguments).
fn split_head_raw(input: &str) -> (&str, &str) {
    let trimmed = input.trim_start();
    if let Some((idx, _)) = trimmed.char_indices().find(|(_, c)| c.is_whitespace()) {
        let (head, tail) = trimmed.split_at(idx);
        (head, tail.trim_start()) // only trim leading whitespace, preserve trailing
    } else {
        (trimmed, "")
    }
}

fn parse_optional_register(input: &str) -> Option<crate::primitives::RegisterName> {
    let trimmed = input.trim();
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    if chars.next().is_none() {
        crate::primitives::RegisterName::new(first)
    } else {
        None
    }
}

fn parse_substitute(input: &str, range: ExRange) -> Result<ExCommand, VimError> {
    let mut chars = input.chars();
    let _ = chars.next();
    let delim = chars.next().ok_or(VimError::InvalidRegexDelimiter)?;
    let remainder = &input[1 + delim.len_utf8()..];

    let (pattern, rest) = take_delimited(remainder, delim)?;
    let (replacement, flags) = take_delimited(rest, delim)?;

    Ok(ExCommand::Substitute {
        range,
        pattern: CompactString::from(pattern),
        replacement: CompactString::from(replacement),
        flags: SubFlags::parse(flags.trim()),
    })
}

fn parse_global(input: &str, invert: bool, range: ExRange) -> Result<ExCommand, VimError> {
    let mut chars = input.chars();
    let _ = chars.next(); // skip 'g' or 'v'
    let delim = chars.next().ok_or(VimError::InvalidSearchPattern)?;
    let remainder = &input[1 + delim.len_utf8()..];

    let (pattern, rest) = take_delimited(remainder, delim)?;
    let nested = rest.trim();
    if nested.is_empty() {
        return Err(VimError::NoCommandAfterGlobal);
    }

    let command = parse_ex_command(nested)?;
    Ok(ExCommand::Global {
        range,
        pattern: CompactString::from(pattern),
        command: Box::new(command),
        invert,
    })
}

/// Like `parse_global` but input starts directly at the delimiter (no command letter prefix).
/// Used by `:g!` where the `g!` prefix has already been stripped.
fn parse_global_from_delim(
    input: &str,
    invert: bool,
    range: ExRange,
) -> Result<ExCommand, VimError> {
    let mut chars = input.chars();
    let delim = chars.next().ok_or(VimError::InvalidSearchPattern)?;
    let remainder = &input[delim.len_utf8()..];

    let (pattern, rest) = take_delimited(remainder, delim)?;
    let nested = rest.trim();
    if nested.is_empty() {
        return Err(VimError::NoCommandAfterGlobal);
    }

    let command = parse_ex_command(nested)?;
    Ok(ExCommand::Global {
        range,
        pattern: CompactString::from(pattern),
        command: Box::new(command),
        invert,
    })
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "returns Result to allow future error paths"
)]
fn take_delimited(input: &str, delim: char) -> Result<(&str, &str), VimError> {
    let mut escaped = false;
    for (idx, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == delim {
            let next = idx + ch.len_utf8();
            return Ok((&input[..idx], &input[next..]));
        }
    }
    // No closing delimiter found — treat remainder as the field content.
    // Vim allows `:s/old/new` without a trailing delimiter.
    Ok((input, ""))
}

fn parse_optional_range(input: &str) -> Result<(Option<ExRange>, &str), VimError> {
    let trimmed = input.trim_start();

    if let Some(rest) = trimmed.strip_prefix('%') {
        return Ok((Some(ExRange::entire_file()), rest));
    }

    if let Some(rest) = trimmed.strip_prefix('*') {
        return Ok((Some(ExRange::visual_selection()), rest));
    }

    let Some((start, consumed)) = parse_line_spec_prefix(trimmed)? else {
        return Ok((None, trimmed));
    };

    let rest = &trimmed[consumed..];
    let rest = rest.trim_start();
    let (separator, after_sep) = if let Some(rest) = rest.strip_prefix(';') {
        (RangeSeparator::Semicolon, rest)
    } else if let Some(rest) = rest.strip_prefix(',') {
        (RangeSeparator::Comma, rest)
    } else {
        return Ok((
            Some(ExRange {
                start,
                end: None,
                separator: RangeSeparator::Comma,
            }),
            rest,
        ));
    };
    let after_sep = after_sep.trim_start();
    let (end, end_len) = parse_line_spec_prefix(after_sep)?.ok_or(VimError::InvalidRange)?;
    let rest = &after_sep[end_len..];
    Ok((
        Some(ExRange {
            start,
            end: Some(end),
            separator,
        }),
        rest,
    ))
}

fn parse_line_spec_exact(input: &str) -> Result<LineSpec, VimError> {
    let trimmed = input.trim();
    let (spec, len) =
        parse_line_spec_prefix(trimmed)?.ok_or_else(|| VimError::InvalidAddress(trimmed.into()))?;
    if len != trimmed.len() {
        return Err(VimError::InvalidAddress(trimmed.into()));
    }
    Ok(spec)
}

fn parse_line_spec_prefix(input: &str) -> Result<Option<(LineSpec, usize)>, VimError> {
    let mut chars = input.char_indices();
    let Some((_, first)) = chars.next() else {
        return Ok(None);
    };
    let (base_spec, base_len) = match first {
        '.' => (LineSpec::Current, 1),
        '$' => (LineSpec::Last, 1),
        '\'' => {
            let Some((idx, mark)) = chars.next() else {
                return Err(VimError::MarkNotSet(' '));
            };
            let Some(mark_name) = MarkName::new(mark) else {
                return Err(VimError::MarkNotSet(mark));
            };
            (LineSpec::Mark(mark_name), idx + mark.len_utf8())
        }
        '/' => {
            let (pattern, rest) = take_delimited(&input[1..], '/')?;
            let consumed = input.len() - rest.len();
            return Ok(Some((
                LineSpec::SearchForward(CompactString::from(pattern)),
                consumed,
            )));
        }
        '?' => {
            let (pattern, rest) = take_delimited(&input[1..], '?')?;
            let consumed = input.len() - rest.len();
            return Ok(Some((
                LineSpec::SearchBackward(CompactString::from(pattern)),
                consumed,
            )));
        }
        '+' | '-' => {
            let sign = if first == '-' { -1 } else { 1 };
            let digits_len = input[1..]
                .chars()
                .take_while(char::is_ascii_digit)
                .map(char::len_utf8)
                .sum::<usize>();
            let digits = &input[1..=digits_len];
            let magnitude = if digits.is_empty() {
                1
            } else {
                digits
                    .parse::<i32>()
                    .map_err(|_| VimError::InvalidAddress(input.into()))?
            };
            return Ok(Some((LineSpec::Relative(sign * magnitude), 1 + digits_len)));
        }
        c if c.is_ascii_digit() => {
            let len = input
                .chars()
                .take_while(char::is_ascii_digit)
                .map(char::len_utf8)
                .sum::<usize>();
            let number = input[..len]
                .parse::<u32>()
                .map_err(|_| VimError::InvalidAddress(input.into()))?;
            (LineSpec::Absolute(number), len)
        }
        _ => return Ok(None),
    };

    // Check for +N/-N offset suffix (e.g. `.+1`, `$-2`, `'a+3`)
    let rest = &input[base_len..];
    if let Some(sign_ch) = rest.chars().next() {
        if sign_ch == '+' || sign_ch == '-' {
            let sign: i32 = if sign_ch == '-' { -1 } else { 1 };
            let digits_len = rest[1..]
                .chars()
                .take_while(char::is_ascii_digit)
                .map(char::len_utf8)
                .sum::<usize>();
            let digits = &rest[1..=digits_len];
            let magnitude = if digits.is_empty() {
                1
            } else {
                digits
                    .parse::<i32>()
                    .map_err(|_| VimError::InvalidAddress(input.into()))?
            };
            let total_consumed = base_len + 1 + digits_len;
            return Ok(Some((
                base_spec.with_offset(sign * magnitude),
                total_consumed,
            )));
        }
    }

    Ok(Some((base_spec, base_len)))
}

/// Strip mapping flag tokens from the start of args, returning flags and remainder.
fn strip_map_flags(input: &str) -> (crate::keymap::MappingFlags, &str) {
    let mut flags = crate::keymap::MappingFlags::default();
    let mut rest = input;
    loop {
        let candidate = rest.trim_start();
        if candidate.starts_with('<') {
            if let Some(end) = candidate.find('>') {
                let token = &candidate[..=end];
                if token.eq_ignore_ascii_case("<nowait>") {
                    flags.nowait = true;
                } else if token.eq_ignore_ascii_case("<expr>") {
                    flags.expr = true;
                } else if token.eq_ignore_ascii_case("<silent>") {
                    flags.silent = true;
                } else {
                    break;
                }
                rest = &candidate[end + 1..];
                continue;
            }
        }
        break;
    }
    (flags, rest.trim_start())
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "returns Result to match parse_named_command dispatch"
)]
fn parse_map_command(
    args: &str,
    mode_prefix: MapModePrefix,
    noremap: bool,
) -> Result<ExCommand, VimError> {
    use crate::keymap::MappingKind;

    let kind = if noremap {
        MappingKind::NonRecursive
    } else {
        MappingKind::Recursive
    };

    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(ExCommand::Map {
            mode_prefix,
            lhs: CompactString::new(""),
            rhs: None,
            kind,
            flags: crate::keymap::MappingFlags::default(),
        });
    }

    let (flags, rest) = strip_map_flags(trimmed);

    // Split into lhs (first token) and rhs (rest)
    let (lhs, rhs) = match rest.find(char::is_whitespace) {
        Some(idx) => (&rest[..idx], rest[idx..].trim_start()),
        None => (rest, ""),
    };
    let rhs = if rhs.is_empty() {
        None
    } else {
        Some(CompactString::from(rhs))
    };
    Ok(ExCommand::Map {
        mode_prefix,
        lhs: CompactString::from(lhs),
        rhs,
        kind,
        flags,
    })
}

fn parse_unmap_command(args: &str, mode_prefix: MapModePrefix) -> Result<ExCommand, VimError> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Err(VimError::ArgumentRequired);
    }
    // lhs is the first token (unmap takes exactly one argument)
    let lhs = trimmed.split_whitespace().next().unwrap_or(trimmed);
    Ok(ExCommand::Unmap {
        mode_prefix,
        lhs: CompactString::from(lhs),
    })
}

/// Parse `:set` assignments from the argument string.
///
/// Supports:
/// - `:set all` → `ShowAll`
/// - `:set expandtab` → `SetBool("expandtab")`
/// - `:set noexpandtab` → `UnsetBool("expandtab")`
/// - `:set expandtab!` → `ToggleBool("expandtab")`
/// - `:set expandtab?` → `Query("expandtab")`
/// - `:set tabstop=8` → `Assign("tabstop", "8")`
fn parse_set_assignments(args: &str) -> smallvec::SmallVec<[SetAssignment; 2]> {
    let mut assignments = smallvec::SmallVec::new();

    if args.is_empty() {
        assignments.push(SetAssignment::ShowAll);
        return assignments;
    }

    for token in args.split_whitespace() {
        if token == "all" {
            assignments.push(SetAssignment::ShowAll);
            continue;
        }

        // `:set name?` — query
        if let Some(name) = token.strip_suffix('?') {
            assignments.push(SetAssignment::Query(CompactString::from(name)));
            continue;
        }

        // `:set name!` — toggle
        if let Some(name) = token.strip_suffix('!') {
            assignments.push(SetAssignment::ToggleBool(CompactString::from(name)));
            continue;
        }

        // `:set name=value` — assignment
        if let Some((name, value)) = token.split_once('=') {
            assignments.push(SetAssignment::Assign(
                CompactString::from(name),
                CompactString::from(value),
            ));
            continue;
        }

        // `:set name:value` — alternate assignment syntax
        if let Some((name, value)) = token.split_once(':') {
            assignments.push(SetAssignment::Assign(
                CompactString::from(name),
                CompactString::from(value),
            ));
            continue;
        }

        // `:set noname` → UnsetBool("name"), `:set name` → SetBool("name").
        // The "no" prefix is stripped here so the grammar layer emits the
        // semantically correct variant.
        if let Some(stripped) = token.strip_prefix("no") {
            if stripped.is_empty() {
                assignments.push(SetAssignment::SetBool(CompactString::from(token)));
            } else {
                assignments.push(SetAssignment::UnsetBool(CompactString::from(stripped)));
            }
        } else {
            assignments.push(SetAssignment::SetBool(CompactString::from(token)));
        }
    }

    assignments
}

/// Parse `:sethandler` arguments.
///
/// IdeaVim-compatible syntax:
/// - `:sethandler <C-A> n:vim i:ide` — key + mode:handler assignments
/// - `:sethandler n:vim i:ide` — no key, global default
///
/// The first token is treated as a key if it starts with `<`.
/// Remaining tokens are `mode-chars:handler` pairs.
fn parse_sethandler(args: &str) -> Result<ExCommand, VimError> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Err(VimError::ArgumentRequired);
    }

    let mut tokens = trimmed.split_whitespace();
    let first = tokens.next().unwrap_or("");

    // If the first token starts with '<', it's a key notation
    let (key, assignments_iter): (Option<CompactString>, Box<dyn Iterator<Item = &str> + '_>) =
        if first.starts_with('<') {
            (Some(CompactString::from(first)), Box::new(tokens))
        } else {
            // No key — first token is a mode:handler assignment
            (None, Box::new(std::iter::once(first).chain(tokens)))
        };

    let mut assignments = Vec::new();
    for token in assignments_iter {
        // Each token is "mode-chars:handler" (e.g., "n:vim", "n-v:ide", "a:host")
        let Some((mode_part, handler_part)) = token.split_once(':') else {
            return Err(VimError::InvalidArgument(CompactString::from(format!(
                "Expected mode:handler, got: {token}"
            ))));
        };
        if mode_part.is_empty() || handler_part.is_empty() {
            return Err(VimError::InvalidArgument(CompactString::from(format!(
                "Empty mode or handler in: {token}"
            ))));
        }
        assignments.push((
            CompactString::from(mode_part),
            CompactString::from(handler_part),
        ));
    }

    if assignments.is_empty() {
        return Err(VimError::ArgumentRequired);
    }

    Ok(ExCommand::SetHandler { key, assignments })
}

/// Parse structural regex commands: `:sx/pattern/command` and `:sy/pattern/command`.
///
/// The `complement` flag distinguishes `:sy` (true) from `:sx` (false).
fn parse_structural(input: &str, complement: bool) -> Result<ExCommand, VimError> {
    // Skip "sx" or "sy"
    let after_cmd = &input[2..];
    let mut chars = after_cmd.chars();
    let delim = chars.next().ok_or(VimError::InvalidRegexDelimiter)?;
    let remainder = &after_cmd[delim.len_utf8()..];

    let (pattern, rest) = take_delimited(remainder, delim)?;
    let nested = rest.trim();
    if nested.is_empty() {
        return Err(VimError::NotEditorCommand(CompactString::from(
            "structural regex: missing sub-command",
        )));
    }

    let sub_command = parse_ex_command(nested)?;
    let flags = crate::primitives::StructuralFlags::default();

    if complement {
        Ok(ExCommand::StructuralComplement {
            pattern: CompactString::from(pattern),
            command: Box::new(sub_command),
            flags,
        })
    } else {
        Ok(ExCommand::StructuralExtract {
            pattern: CompactString::from(pattern),
            command: Box::new(sub_command),
            flags,
        })
    }
}

/// Parse multi-cursor selection ex commands.
///
/// Returns `Ok(Some(cmd))` if the input matches a multi-cursor ex command,
/// `Ok(None)` if it does not match (should fall through to normal parsing),
/// or `Err` if the syntax is recognisable but malformed.
fn parse_multi_cursor_ex(
    rest: &str,
    range: Option<ExRange>,
) -> Result<Option<ExCommand>, VimError> {
    let (head, tail) = split_head(rest);
    let name = head.trim_end_matches('!');

    // :select /pattern/ — sel[ect]
    if matches_abbrev(name, "sel", "select") {
        // Disambiguate from `:set selection=...` which is `se[t]` — "sel" is 3 chars,
        // longer than "set" (3 chars) so no conflict in practice since `set` is matched
        // as `se` minimum. But "sel" could match "set" too — checked: matches_abbrev
        // compares "sel"[..3] == "set"[..3] → "sel" != "set" → no match. Safe.
        let pattern = parse_pattern_arg(tail)?;
        return Ok(Some(ExCommand::SelectMatches { range, pattern }));
    }

    // :split /pattern/ — sp[lit] with a delimiter arg → SplitMatches
    // (without delimiter → falls through to window Split via parse_named_command)
    if matches_abbrev(name, "sp", "split") {
        let trimmed = tail.trim_start();
        // If tail starts with a non-alphanumeric, non-space char → treat as pattern delimiter
        if let Some(first) = trimmed.chars().next() {
            if !first.is_alphanumeric() && !first.is_whitespace() && first != '!' {
                let pattern = parse_pattern_arg(tail)?;
                return Ok(Some(ExCommand::SplitMatches { range, pattern }));
            }
        }
        // Otherwise fall through to window :split
        return Ok(None);
    }

    // :keep /pattern/ — kee[p]
    if matches_abbrev(name, "kee", "keep") {
        let pattern = parse_pattern_arg(tail)?;
        return Ok(Some(ExCommand::KeepMatches { range, pattern }));
    }

    // :remove /pattern/ — remo[ve]
    // Use "remo" as min to avoid conflicts with `:read` (r), `:registers` (reg), `:retab` (ret), `:right` (ri)
    if matches_abbrev(name, "remo", "remove") {
        let pattern = parse_pattern_arg(tail)?;
        return Ok(Some(ExCommand::RemoveMatches { range, pattern }));
    }

    // :trim — tri[m]
    if matches_abbrev(name, "tri", "trim") {
        return Ok(Some(ExCommand::TrimSelections));
    }

    // :align — ali[gn]
    if matches_abbrev(name, "ali", "align") {
        return Ok(Some(ExCommand::AlignSelections));
    }

    // :rotate — rot[ate]
    if matches_abbrev(name, "rot", "rotate") {
        return Ok(Some(ExCommand::RotateContents));
    }

    // ── New multi-cursor ex commands ──────────────────────────────

    let force = head.ends_with('!');

    // :addnext [count] — addn[ext]
    if matches_abbrev(name, "addn", "addnext") {
        let count = tail.trim().parse::<u32>().ok();
        return Ok(Some(ExCommand::AddNext { count }));
    }

    // :addprev [count] — addp[rev]
    if matches_abbrev(name, "addp", "addprev") {
        let count = tail.trim().parse::<u32>().ok();
        return Ok(Some(ExCommand::AddPrev { count }));
    }

    // :skipmatch — skip[match]
    if matches_abbrev(name, "skip", "skipmatch") {
        return Ok(Some(ExCommand::SkipMatch));
    }

    // :addcursor above|below [count] — addc[ursor]
    if matches_abbrev(name, "addc", "addcursor") {
        let args: Vec<&str> = tail.split_whitespace().collect();
        let direction = match args.first().copied() {
            Some("above" | "up") => crate::primitives::Direction::Backward,
            _ => crate::primitives::Direction::Forward, // "below", "down", or default
        };
        let count = args.get(1).and_then(|s| s.parse::<u32>().ok());
        return Ok(Some(ExCommand::AddCursorDir { direction, count }));
    }

    // :selectall — selecta[ll]
    if matches_abbrev(name, "selecta", "selectall") {
        return Ok(Some(ExCommand::SelectAll));
    }

    // :cursorfilter[!] {pattern} — cursorf[ilter]
    // Must come before cursorcollapse/cursorflip/cursorforward to avoid prefix conflicts.
    if matches_abbrev(name, "cursorf", "cursorfilter") {
        let pattern = parse_pattern_arg(tail)?;
        if force {
            return Ok(Some(ExCommand::RemoveMatches { range, pattern }));
        }
        return Ok(Some(ExCommand::KeepMatches { range, pattern }));
    }

    // :cursorflip — cursorfl[ip]
    if matches_abbrev(name, "cursorfl", "cursorflip") {
        return Ok(Some(ExCommand::CursorFlip));
    }

    // :cursorforward — cursorfo[rward]
    if matches_abbrev(name, "cursorfo", "cursorforward") {
        return Ok(Some(ExCommand::CursorForward));
    }

    // :cursorcollapse — cursorco[llapse]
    if matches_abbrev(name, "cursorco", "cursorcollapse") {
        return Ok(Some(ExCommand::CursorCollapse));
    }

    // :cursormerge — cursorm[erge]
    if matches_abbrev(name, "cursorm", "cursormerge") {
        return Ok(Some(ExCommand::CursorMerge));
    }

    // :cursorprimary next|prev — cursorp[rimary]
    if matches_abbrev(name, "cursorp", "cursorprimary") {
        let direction = if tail.trim().starts_with("prev") || tail.trim().starts_with('p') {
            crate::primitives::Direction::Backward
        } else {
            crate::primitives::Direction::Forward // "next" or default
        };
        return Ok(Some(ExCommand::CursorPrimary { direction }));
    }

    // :cursorremove — cursorrem[ove]
    if matches_abbrev(name, "cursorrem", "cursorremove") {
        return Ok(Some(ExCommand::CursorRemove));
    }

    // :cursorrotate [fwd|bwd] — cursorrot[ate]
    // Supports explicit direction: fwd (default) or bwd.
    if matches_abbrev(name, "cursorrot", "cursorrotate") {
        let trimmed_tail = tail.trim();
        if trimmed_tail.is_empty() {
            return Ok(Some(ExCommand::RotateContents));
        }
        let direction = if trimmed_tail.starts_with("bwd") || trimmed_tail.starts_with("back") {
            crate::primitives::Direction::Backward
        } else {
            crate::primitives::Direction::Forward
        };
        return Ok(Some(ExCommand::RotateContentsDir { direction }));
    }

    // :cursorsplit — cursorsp[lit]
    // Must come before cursorsplitsel to check cursorsp before cursorsplits.
    if matches_abbrev(name, "cursorsp", "cursorsplit")
        && !matches_abbrev(name, "cursorsplits", "cursorsplitsel")
    {
        return Ok(Some(ExCommand::CursorSplitBlock));
    }

    // :cursorsplitsel {pattern} — cursorsplits[el]
    if matches_abbrev(name, "cursorsplits", "cursorsplitsel") {
        let pattern = parse_pattern_arg(tail)?;
        return Ok(Some(ExCommand::SplitMatches { range, pattern }));
    }

    // :cursorselect {pattern} — cursorsel[ect]
    if matches_abbrev(name, "cursorsel", "cursorselect") {
        let pattern = parse_pattern_arg(tail)?;
        return Ok(Some(ExCommand::SelectMatches { range, pattern }));
    }

    // :cursortrim — cursort[rim]
    if matches_abbrev(name, "cursort", "cursortrim") {
        return Ok(Some(ExCommand::TrimSelections));
    }

    // :cursoralign — cursora[lign]
    if matches_abbrev(name, "cursora", "cursoralign") {
        return Ok(Some(ExCommand::AlignSelections));
    }

    Ok(None)
}

/// Parse a delimited pattern argument from the tail of an ex command.
///
/// Expects `tail` to contain a pattern like `/foo/` or `|bar|`. The first
/// non-whitespace character is the delimiter.
///
/// Returns the pattern string between the delimiters.
///
/// # Errors
///
/// Returns `VimError::InvalidSearchPattern` if the tail is empty or contains
/// only whitespace.
fn parse_pattern_arg(tail: &str) -> Result<String, VimError> {
    let trimmed = tail.trim_start();
    if trimmed.is_empty() {
        return Err(VimError::InvalidSearchPattern);
    }

    let mut chars = trimmed.chars();
    let delim = chars.next().unwrap(); // trimmed is non-empty
    let remainder = &trimmed[delim.len_utf8()..];

    let (pattern, _) = take_delimited(remainder, delim)?;
    Ok(pattern.to_owned())
}

/// Parse `:z` command: `:[line]z[+-=.^#] [count]`
///
/// `style_char` is provided when `split_head` attached the modifier to the command
/// name (e.g., `"z."` splits to name=`"z."`, so the caller extracts `Some(b'.')`).
fn parse_z_command(
    style_char: Option<u8>,
    tail: &str,
    range: ExRange,
) -> Result<ExCommand, VimError> {
    use crate::grammar::types::ZWindowStyle;

    let (style, rest) = if let Some(ch) = style_char {
        let s = match ch {
            b'+' => ZWindowStyle::Below,
            b'-' => ZWindowStyle::Above,
            b'.' => ZWindowStyle::Centered,
            b'=' => ZWindowStyle::Highlighted,
            b'^' => ZWindowStyle::PrevWindow,
            b'#' => ZWindowStyle::Numbered,
            _ => ZWindowStyle::Below,
        };
        (s, tail)
    } else {
        let trimmed = tail.trim_start();
        match trimmed.as_bytes().first() {
            Some(b'+') => (ZWindowStyle::Below, &trimmed[1..]),
            Some(b'-') => (ZWindowStyle::Above, &trimmed[1..]),
            Some(b'.') => (ZWindowStyle::Centered, &trimmed[1..]),
            Some(b'=') => (ZWindowStyle::Highlighted, &trimmed[1..]),
            Some(b'^') => (ZWindowStyle::PrevWindow, &trimmed[1..]),
            Some(b'#') => (ZWindowStyle::Numbered, &trimmed[1..]),
            _ => (ZWindowStyle::Below, trimmed),
        }
    };

    let count = rest.trim().parse::<u32>().ok();

    Ok(ExCommand::ZWindow {
        range,
        style,
        count,
    })
}

/// Parse `:abbreviate` / `:iabbrev` / `:cabbrev` / `:noreabbrev` etc.
///
/// Arguments are whitespace-separated: `[trigger] [replacement ...]`.
#[allow(
    clippy::unnecessary_wraps,
    reason = "returns Result to match parse_named_command dispatch"
)]
fn parse_abbreviate_command(
    args: &str,
    mode: AbbrevMode,
    noremap: bool,
) -> Result<ExCommand, VimError> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Ok(ExCommand::Abbreviate {
            trigger: None,
            replacement: None,
            mode,
            noremap,
        });
    }

    // Split into trigger (first token) and replacement (rest).
    let (trigger, rhs) = match trimmed.find(char::is_whitespace) {
        Some(idx) => (&trimmed[..idx], trimmed[idx..].trim_start()),
        None => (trimmed, ""),
    };

    let replacement = if rhs.is_empty() {
        None
    } else {
        Some(CompactString::from(rhs))
    };

    Ok(ExCommand::Abbreviate {
        trigger: Some(CompactString::from(trigger)),
        replacement,
        mode,
        noremap,
    })
}

/// Parse `:unabbreviate` / `:iunabbrev` / `:cunabbrev`.
fn parse_unabbreviate_command(args: &str, mode: AbbrevMode) -> Result<ExCommand, VimError> {
    let trimmed = args.trim();
    if trimmed.is_empty() {
        return Err(VimError::ArgumentRequired);
    }
    // Trigger is the first token.
    let trigger = trimmed.split_whitespace().next().unwrap_or(trimmed);
    Ok(ExCommand::Unabbreviate {
        trigger: CompactString::from(trigger),
        mode,
    })
}

/// Split an ex command line into individual commands separated by unescaped `|`.
///
/// The `|` character does NOT split inside:
/// - Regex patterns delimited by `/` or `?`
/// - `:!` shell commands (everything after `!` is one command)
///
/// Returns a `Vec` of individual command strings (trimmed).
#[must_use]
pub fn split_ex_pipeline(input: &str) -> Vec<&str> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    // If the (non-range) command part starts with `!`, don't split at all --
    // the entire line is a shell command where `|` is meaningful.
    if is_bang_command(trimmed) {
        return vec![trimmed];
    }

    let mut commands = Vec::new();
    let mut start = 0;
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            '|' => {
                let segment = input[start..i].trim();
                if !segment.is_empty() {
                    commands.push(segment);
                }
                start = i + 1;
            }
            _ => {
                // Check if we're at the start of a command that uses
                // delimited arguments (s, g, v followed by a non-alnum).
                // If so, skip past all its delimiters to avoid splitting
                // on `|` embedded inside patterns/replacements.
                let cmd_start = find_command_start(input, start, i);
                if let Some(new_i) = skip_delimited_command(bytes, cmd_start, i) {
                    i = new_i;
                }
            }
        }
        i += 1;
    }

    // Last command
    let last = input[start..].trim();
    if !last.is_empty() {
        commands.push(last);
    }

    commands
}

/// Find the position of the command name relative to the current segment.
///
/// Skips past optional range prefixes (digits, `.`, `$`, `%`, marks,
/// `+`, `-`, `,`, `;`, whitespace) to locate the first command character.
fn find_command_start(input: &str, segment_start: usize, current: usize) -> usize {
    let segment = &input[segment_start..=current];
    let trimmed = segment.trim_start();
    let offset = segment.len() - trimmed.len();
    let base = segment_start + offset;

    // Skip range characters to find the actual command letter.
    let mut j = base;
    let bytes = input.as_bytes();
    while j <= current {
        match bytes[j] {
            b'0'..=b'9' | b'.' | b'$' | b'%' | b',' | b';' | b'+' | b'-' | b' ' | b'\t' => {
                j += 1;
            }
            b'\'' if j + 1 < bytes.len() => {
                j += 2; // mark 'x
            }
            _ => break,
        }
    }
    j
}

/// If position `i` is the command letter of `s`, `g`, or `v` followed by a
/// delimiter, skip past all the delimited fields. Returns the new scan
/// position (pointing at the last consumed character), or `None` if this
/// is not a delimited command.
fn skip_delimited_command(bytes: &[u8], cmd_start: usize, i: usize) -> Option<usize> {
    if i != cmd_start {
        return None;
    }

    let cmd = bytes[i];
    // s/g/v must be followed by a non-alphanumeric, non-underscore delimiter.
    let next = bytes.get(i + 1)?;
    if next.is_ascii_alphanumeric() || *next == b'_' {
        return None;
    }

    let delim = *next;
    let delim_count = match cmd {
        b's' => 3,        // s{d}pattern{d}replacement{d}
        b'g' | b'v' => 2, // g{d}pattern{d}  (the nested command follows)
        _ => return None,
    };

    // Skip past `delim_count` unescaped delimiters starting from the first one.
    let mut pos = i + 1; // at the first delimiter
    let mut seen = 0;
    while pos < bytes.len() && seen < delim_count {
        if bytes[pos] == delim {
            seen += 1;
        } else if bytes[pos] == b'\\' && pos + 1 < bytes.len() {
            pos += 1; // skip escaped char
        }
        pos += 1;
    }

    if seen >= delim_count {
        // pos is now just past the last delimiter. Back up by 1 because
        // the outer loop will do i += 1.
        Some(pos - 1)
    } else {
        // Fewer delimiters than expected -- consume to where we got.
        Some(pos.saturating_sub(1))
    }
}

/// Check whether the command portion (after optional range) starts with `!`.
fn is_bang_command(input: &str) -> bool {
    let trimmed = input.trim_start();
    // Skip past an optional range prefix to find the actual command.
    // Ranges start with digits, `.`, `$`, `%`, `'`, `+`, `-`, `/`, `?`.
    let mut rest = trimmed;
    loop {
        let ch = match rest.bytes().next() {
            Some(c) => c,
            None => return false,
        };
        match ch {
            b'0'..=b'9' => {
                rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            }
            b'.' | b'$' | b'%' => {
                rest = &rest[1..];
            }
            b'+' | b'-' => {
                rest = &rest[1..];
                rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());
            }
            b'\'' => {
                // Mark: skip 'x
                if rest.len() >= 2 {
                    rest = &rest[2..];
                } else {
                    return false;
                }
            }
            b',' | b';' => {
                // Range separator -- continue to parse the second address.
                rest = &rest[1..];
                rest = rest.trim_start();
            }
            b' ' | b'\t' => {
                rest = rest.trim_start();
            }
            _ => break,
        }
    }
    rest.starts_with('!')
}

#[cfg(test)]
#[path = "ex_parser_tests.rs"]
mod tests;
