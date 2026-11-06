#!/usr/bin/env python3
"""Generate vim-core's tables of Vim's commands and options from a Vim source
tree.

    python3 scripts/gen-vim-tables.py <vim-source-dir>

The Vim tree must be a git checkout at a release tag. The script reads
src/ex_cmds.h (the Ex command table, its EX_* flags and address types), the
cmdmods[] table of src/ex_docmd.c and the event_names[] table of
src/autocmd.c, and writes vim-core/src/grammar/vim_commands.rs. It reads the
options[] table of src/optiondefs.h and writes
vim-core/src/primitives/vim_option_names.rs. The output is checked in;
regenerating it is a deliberate change.
"""

import pathlib
import re
import subprocess
import sys

EXPECTED_COMMANDS = 594
EXPECTED_MODIFIERS = 26
EXPECTED_EVENTS = 117
EXPECTED_OPTIONS = 436
EXPECTED_SECURE = 62
EXPECTED_TERMINAL_CODES = 89

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "vim-core/src/grammar/vim_commands.rs"
OPTIONS_OUT = ROOT / "vim-core/src/primitives/vim_option_names.rs"


def strip_comments(text):
    text = re.sub(r"/\*.*?\*/", " ", text, flags=re.S)
    return re.sub(r"//[^\n]*", " ", text)


def camel(name):
    return "".join(part.capitalize() for part in name.lower().split("_"))


def vim_tag(src):
    try:
        out = subprocess.run(
            ["git", "-C", str(src), "describe", "--tags", "--exact-match"],
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        sys.exit(f"{src} is not a git checkout at a release tag")
    return out.stdout.strip()


def parse_ex_cmds(path):
    text = strip_comments(path.read_text(encoding="latin-1"))

    flags = {}
    for name, value in re.findall(r"#define\s+EX_(\w+)\s+(0x[0-9a-fA-F]+)", text):
        flags[name] = int(value, 16)
    composite = {}
    for name, expr in re.findall(r"#define\s+EX_(\w+)\s+\(([^)]*)\)", text):
        composite[name] = [part.strip().removeprefix("EX_") for part in expr.split("|")]

    addr = re.search(r"typedef enum\s*\{([^}]*)\}\s*cmd_addr_T;", text)
    if addr is None:
        sys.exit("cmd_addr_T not found in ex_cmds.h")
    addr_types = [a.strip().removeprefix("ADDR_") for a in addr.group(1).split(",") if a.strip()]

    rows = []
    pattern = re.compile(
        r'EXCMD\(\s*CMD_\w+\s*,\s*"([^"]*)"\s*,\s*\w+\s*,\s*([^,]*?)\s*,\s*ADDR_(\w+)\s*\)'
    )
    for name, flag_expr, addr_type in pattern.findall(text):
        names = [f.strip().removeprefix("EX_") for f in flag_expr.split("|")]
        for f in names:
            if f not in flags and f not in composite:
                sys.exit(f"unknown flag EX_{f} on :{name}")
        if addr_type not in addr_types:
            sys.exit(f"unknown address type ADDR_{addr_type} on :{name}")
        rows.append((name, names, addr_type))
    if len(rows) != EXPECTED_COMMANDS:
        sys.exit(f"found {len(rows)} commands, expected {EXPECTED_COMMANDS}")
    return flags, composite, addr_types, rows


def parse_cmdmods(path):
    text = strip_comments(path.read_text(encoding="latin-1"))
    table = re.search(r"cmdmods\[\]\s*=\s*\{(.*?)\};", text, flags=re.S)
    if table is None:
        sys.exit("cmdmods[] not found in ex_docmd.c")
    mods = re.findall(r'\{\s*"(\w+)"\s*,\s*(\d+)\s*,\s*(TRUE|FALSE)\s*\}', table.group(1))
    if len(mods) != EXPECTED_MODIFIERS:
        sys.exit(f"found {len(mods)} modifiers, expected {EXPECTED_MODIFIERS}")
    return [(name, int(minlen), has_count == "TRUE") for name, minlen, has_count in mods]


def parse_events(path):
    text = strip_comments(path.read_text(encoding="latin-1"))
    table = re.search(r"event_names\[\]\s*=\s*\{(.*?)\};", text, flags=re.S)
    if table is None:
        sys.exit("event_names[] not found in autocmd.c")
    events = re.findall(r'\{\s*"(\w+)"\s*,\s*EVENT_\w+\s*\}', table.group(1))
    if len(events) != EXPECTED_EVENTS:
        sys.exit(f"found {len(events)} events, expected {EXPECTED_EVENTS}")
    return events


def parse_options(path):
    text = strip_comments(path.read_text(encoding="latin-1"))
    table = re.search(r"static struct vimoption options\[\]\s*=\s*\{(.*?)\n\};", text, flags=re.S)
    if table is None:
        sys.exit("options[] not found in optiondefs.h")
    body = table.group(1)
    rows = []
    for name, short, flag_text in re.findall(
        r'\{\s*"(\w+)"\s*,\s*(?:"(\w+)"|NULL)\s*,([^,]*),', body
    ):
        # A flag may sit inside #if lines (guifont).
        flag_text = "\n".join(
            line for line in flag_text.splitlines() if not line.lstrip().startswith("#")
        )
        names = re.findall(r"P_\w+", flag_text)
        kinds = [f for f in names if f in ("P_BOOL", "P_NUM", "P_STRING")]
        if len(kinds) != 1:
            sys.exit(f"option {name} has no single type: {names}")
        kind = {"P_BOOL": "Bool", "P_NUM": "Number", "P_STRING": "String"}[kinds[0]]
        rows.append((name, short, kind, "P_SECURE" in names))
    if len(rows) != EXPECTED_OPTIONS:
        sys.exit(f"found {len(rows)} options, expected {EXPECTED_OPTIONS}")
    secure = sum(1 for row in rows if row[3])
    if secure != EXPECTED_SECURE:
        sys.exit(f"found {secure} P_SECURE options, expected {EXPECTED_SECURE}")
    macro = re.search(r"#define\s+p_term\(sss,\s*vvv\)(.*?)SCTX_INIT\}", text, flags=re.S)
    if macro is None or "P_STRING" not in macro.group(1) or "P_SECURE" not in macro.group(1):
        sys.exit("p_term() in optiondefs.h is no longer a P_STRING|P_SECURE option")
    codes = re.findall(r'^\s*p_term\(\s*"([^"]+)"\s*,', body, flags=re.M)
    if len(codes) != EXPECTED_TERMINAL_CODES:
        sys.exit(f"found {len(codes)} terminal codes, expected {EXPECTED_TERMINAL_CODES}")
    return rows, codes


def render_options(tag, rows, codes):
    out = []
    w = out.append
    w("//! Vim's option table at tag " + tag + ", from `options[]` in")
    w("//! `src/optiondefs.h`.")
    w("//!")
    w("//! Generated; do not edit. Regenerate with")
    w("//! `python3 scripts/gen-vim-tables.py <vim-source-dir>`.")
    w("")
    w("/// The type of an option's value: `P_BOOL`, `P_NUM` or `P_STRING`.")
    w("#[derive(Debug, Clone, Copy, PartialEq, Eq)]")
    w("pub(crate) enum OptionType {")
    w("    Bool,")
    w("    Number,")
    w("    String,")
    w("}")
    w("")
    w("/// One row of Vim's option table.")
    w("#[derive(Debug, Clone, Copy)]")
    w("pub(crate) struct VimOption {")
    w("    /// The full name.")
    w("    pub(crate) name: &'static str,")
    w("    /// The short name, or `\"\"` for none.")
    w("    pub(crate) short: &'static str,")
    w("    /// The type of its value.")
    w("    pub(crate) kind: OptionType,")
    w("    /// `P_SECURE`: a modeline and secure mode cannot set it.")
    w("    #[allow(dead_code, reason = \"the whole of Vim's table\")]")
    w("    pub(crate) secure: bool,")
    w("}")
    w("")
    w("const fn o(name: &'static str, short: &'static str, kind: OptionType, secure: bool) -> VimOption {")
    w("    VimOption {")
    w("        name,")
    w("        short,")
    w("        kind,")
    w("        secure,")
    w("    }")
    w("}")
    w("")
    w("/// Every option but the terminal codes, in Vim's order, which is")
    w("/// alphabetic by full name.")
    w("#[rustfmt::skip]")
    w(f"pub(crate) static OPTIONS: [VimOption; {len(rows)}] = {{")
    w("    use OptionType::{Bool, Number, String};")
    w("    [")
    for name, short, kind, secure in rows:
        w(f'        o("{name}", "{short}", {kind}, {"true" if secure else "false"}),')
    w("    ]")
    w("};")
    w("")
    w("/// The terminal output codes (`t_xx`), each a `P_STRING` and `P_SECURE`")
    w("/// option.")
    w("#[rustfmt::skip]")
    w(f"pub(crate) static TERMINAL_CODES: [&str; {len(codes)}] = [")
    for code in codes:
        w(f'    "{code}",')
    w("];")
    return "\n".join(out) + "\n"


def render(tag, flags, composite, addr_types, rows, mods, events):
    out = []
    w = out.append
    w("//! Vim's Ex command table, command modifiers and autocommand events at")
    w(f"//! tag {tag}, from `src/ex_cmds.h`, `cmdmods[]` in `src/ex_docmd.c` and")
    w("//! `event_names[]` in `src/autocmd.c`.")
    w("//!")
    w("//! Generated; do not edit. Regenerate with")
    w("//! `python3 scripts/gen-vim-tables.py <vim-source-dir>`.")
    w("")
    w("/// Vim's `EX_*` command flags.")
    w("#[allow(dead_code, reason = \"the whole of Vim's flag set\")]")
    w("pub(crate) mod ex {")
    for name, value in flags.items():
        digits = f"{value:08x}"
        w(f"    pub(crate) const {name}: u32 = 0x{digits[:4]}_{digits[4:]};")
    for name, parts in composite.items():
        w(f"    pub(crate) const {name}: u32 = {' | '.join(parts)};")
    w("}")
    w("")
    w("/// Vim's `cmd_addr_T`: how a command reads its range.")
    w("#[derive(Debug, Clone, Copy, PartialEq, Eq)]")
    w("pub(crate) enum Addr {")
    for a in addr_types:
        w(f"    {camel(a)},")
    w("}")
    w("")
    w("/// One row of Vim's command table.")
    w("#[derive(Debug, Clone, Copy)]")
    w("pub(crate) struct VimCommand {")
    w("    /// The full name; any prefix of it can find the row.")
    w("    pub(crate) name: &'static str,")
    w("    /// `EX_*` flags.")
    w("    pub(crate) flags: u32,")
    w("    /// How the range is read.")
    w("    pub(crate) addr: Addr,")
    w("}")
    w("")
    w("const fn c(name: &'static str, flags: u32, addr: Addr) -> VimCommand {")
    w("    VimCommand { name, flags, addr }")
    w("}")
    w("")
    w("/// Every command, in Vim's table order, which decides what an")
    w("/// abbreviation means.")
    w("#[rustfmt::skip]")
    w("#[allow(clippy::wildcard_imports, clippy::enum_glob_use, reason = \"bare names keep one row per line\")]")
    w(f"pub(crate) static COMMANDS: [VimCommand; {len(rows)}] = {{")
    w("    use ex::*;")
    w("    use Addr::*;")
    w("    [")
    for name, names, addr_type in rows:
        w(f'        c("{name}", {" | ".join(names)}, {camel(addr_type)}),')
    w("    ]")
    w("};")
    w("")
    w("/// The command modifiers as `(name, shortest form, takes a count)`.")
    w("#[rustfmt::skip]")
    w(f"pub(crate) static MODIFIERS: [(&str, usize, bool); {len(mods)}] = [")
    for name, minlen, has_count in mods:
        w(f'    ("{name}", {minlen}, {"true" if has_count else "false"}),')
    w("];")
    w("")
    w("/// The autocommand event names, aliases included; Vim matches them in any")
    w("/// case.")
    w("#[rustfmt::skip]")
    w(f"pub(crate) static EVENTS: [&str; {len(events)}] = [")
    for name in events:
        w(f'    "{name}",')
    w("];")
    return "\n".join(out) + "\n"


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    src = pathlib.Path(sys.argv[1])
    tag = vim_tag(src)
    flags, composite, addr_types, rows = parse_ex_cmds(src / "src/ex_cmds.h")
    mods = parse_cmdmods(src / "src/ex_docmd.c")
    events = parse_events(src / "src/autocmd.c")
    options, codes = parse_options(src / "src/optiondefs.h")
    OUT.write_text(render(tag, flags, composite, addr_types, rows, mods, events), encoding="utf-8")
    print(f"wrote {OUT}: {len(rows)} commands, {len(mods)} modifiers, {len(events)} events ({tag})")
    OPTIONS_OUT.write_text(render_options(tag, options, codes), encoding="utf-8")
    print(f"wrote {OPTIONS_OUT}: {len(options)} options, {len(codes)} terminal codes ({tag})")


if __name__ == "__main__":
    main()
