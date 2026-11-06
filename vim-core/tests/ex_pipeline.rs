//! A `|` line runs each command on the text the command before it left, and
//! `:g` runs its commands one marked line at a time, as Vim's
//! `do_cmdline()` and `global_exe()` do.

use vim_core::effects::{Effect, InfoMessage};
use vim_core::primitives::MotionType;
use vim_test::session::TestSession;

/// Feed `keys` to `text` with the cursor at its start.
fn run(text: &str, keys: &str) -> TestSession {
    let mut session = TestSession::new(&format!("|{text}"));
    session.feed(keys);
    session
}

fn register(session: &TestSession, name: char) -> Option<String> {
    session.register(name).map(|(text, _)| text)
}

/// `:d` deletes the cursor line and leaves the cursor on the line after it,
/// so the second `:d` deletes that one (ex_docmd.c `do_cmdline()`,
/// ex_docmd.c `ex_operators()`).
#[test]
fn delete_twice_deletes_two_lines() {
    let s = run("abc é\nx€y 😀\nfoo bar\n", ":d|d<CR>");
    assert_eq!(s.text(), "foo bar\n");
    assert_eq!(s.cursor_offset(), 0);
    assert_eq!(register(&s, '"').as_deref(), Some("x€y 😀\n"));
    assert_eq!(register(&s, '1').as_deref(), Some("x€y 😀\n"));
    assert_eq!(register(&s, '2').as_deref(), Some("abc é\n"));

    let s = run("a\nbbb\ncccc\n", ":d|d<CR>");
    assert_eq!(s.text(), "cccc\n");
    assert_eq!(s.cursor_offset(), 0);
}

/// `:s` changes the line and `:d` then deletes the changed line, the only
/// one, which leaves an empty buffer (ex_cmds.c `ex_substitute()`,
/// ops.c `op_delete()`).
#[test]
fn substitute_then_delete_deletes_the_changed_line() {
    for new in ["b€", "b😀", "bｘ", "b\u{200b}", "XXXX"] {
        let s = run("foo bar", &format!(":s/a/{new}/|d<CR>"));
        assert_eq!(s.text(), "", "{new:?}");
        let deleted = format!("foo b{new}r\n");
        assert_eq!(register(&s, '"'), Some(deleted.clone()), "{new:?}");
        assert_eq!(
            s.register('"').map(|(_, kind)| kind),
            Some(MotionType::LineWise)
        );

        let s = run("foo bar\nnext", &format!(":s/a/{new}/|d<CR>"));
        assert_eq!(s.text(), "next", "{new:?}");
        assert_eq!(register(&s, '"'), Some(deleted), "{new:?}");
    }
}

/// The second `:s` sees the first one's change; `u` undoes the whole line
/// (ex_cmds.c `do_sub()`, undo.c `u_savecommon()`: no `u_sync()` between
/// the commands of a line).
#[test]
fn two_substitutes_are_one_undo_step() {
    let mut s = run("foo bar", ":s/o/é/|s/r/x/<CR>");
    assert_eq!(s.text(), "féo bax");
    assert_eq!(s.cursor_offset(), 0);
    s.feed("u");
    assert_eq!(s.text(), "foo bar");

    let mut s = run("foo bar\nzz", ":s/a/b/|d<CR>");
    assert_eq!(s.text(), "zz");
    s.feed("u");
    assert_eq!(s.text(), "foo bar\nzz");
    s.feed("<C-r>");
    assert_eq!(s.text(), "zz");
}

/// `:undo` after a change of the same line undoes that change: `u_undo()`
/// syncs undo first (undo.c `u_undo()`).
#[test]
fn undo_in_a_line_undoes_the_line_so_far() {
    let mut s = run("a\nb\nc", ":d|undo<CR>");
    assert_eq!(s.text(), "a\nb\nc");
    s.feed("<C-r>");
    assert_eq!(s.text(), "b\nc");
}

/// `:2` moves the cursor and the `:d` after it deletes line 2
/// (ex_docmd.c `do_one_cmd()`, a range alone).
#[test]
fn a_line_number_moves_the_cursor_for_the_next_command() {
    let s = run("a\nbbb\ncccc\n", ":2 | d<CR>");
    assert_eq!(s.text(), "a\ncccc\n");
    assert_eq!(s.cursor_offset(), 2);
    assert_eq!(register(&s, '"').as_deref(), Some("bbb\n"));
}

/// An error ends a typed line (ex_docmd.c `do_cmdline()`, the loop
/// condition on `did_emsg` with `getexline`).
#[test]
fn an_error_ends_a_typed_line() {
    let s = run("foo\nbar", ":s/zzz/y/|d<CR>");
    assert_eq!(s.text(), "foo\nbar");
    assert!(s.errmsg().is_some_and(|e| e.starts_with("E486")));
}

/// `:@r` runs the register's lines one after the other, each on the text
/// the one before left (ops.c `do_execreg()`).
#[test]
fn register_lines_run_in_turn() {
    let s = run("1d\n1d\nkeep\n", "\"ryj:@r<CR>");
    assert_eq!(s.text(), "keep\n");
}

/// `:g` marks every line first, then `:m0` moves each marked line to the
/// top in turn, which reverses them; the empty line after the last `\n`
/// is a line too (ex_cmds.c `ex_global()`, `global_exe()`, `do_move()`).
#[test]
fn global_move_to_top_reverses_the_lines() {
    let mut s = run("a\nbbb\ncccc\n", ":g/^/m0<CR>");
    assert_eq!(s.text(), "\ncccc\nbbb\na");
    assert_eq!(s.cursor_offset(), 0);
    s.feed("u");
    assert_eq!(s.text(), "a\nbbb\ncccc\n");

    let s = run("ab\ncd\n😀", ":g/^/m0<CR>");
    assert_eq!(s.text(), "😀\ncd\nab");
    assert_eq!(s.cursor_offset(), 0);

    let s = run("a\nbbb\ncccc\n", ":g/./m0<CR>");
    assert_eq!(s.text(), "cccc\nbbb\na\n");
    assert_eq!(s.cursor_offset(), 0);
}

/// Each line goes to the end in turn, so the text comes back the same and
/// the cursor is on the last line moved (ex_cmds.c `do_move()`).
#[test]
fn global_move_to_end_keeps_the_order() {
    let s = run("a\nbbb\ncccc\n", ":g/^/m$<CR>");
    assert_eq!(s.text(), "a\nbbb\ncccc\n");
    assert_eq!(s.cursor_offset(), 11);
}

/// `:g` runs the whole rest of the line on each line: `:s` that finds
/// nothing is no error inside `:g`, and the cursor goes to the first
/// non-blank after a substitution (ex_cmds.c `do_sub()` with `global_busy`,
/// `global_exe()` with `global_need_beginline`).
#[test]
fn global_runs_every_command_of_the_rest_of_the_line() {
    let mut s = run("xa\nb\nxaa\nxb\n", ":g/x/s/a/b/|d<CR>");
    assert_eq!(s.text(), "b\n");
    assert_eq!(s.cursor_offset(), 2);
    assert_eq!(s.errmsg(), None);
    s.feed("u");
    assert_eq!(s.text(), "xa\nb\nxaa\nxb\n");
}

/// A `:g` inside a `:g` runs on the cursor line when it matches
/// (ex_cmds.c `ex_global()` with `global_busy`).
#[test]
fn a_global_inside_a_global_checks_the_cursor_line() {
    let s = run(
        "foo only\nbar only\nfoo bar both\nneither\nfoo bar again\n",
        ":g/foo/g/bar/d<CR>",
    );
    assert_eq!(s.text(), "foo only\nbar only\nneither\n");
}

/// A range on a `:g` inside a `:g` is E147, and an error ends `:g` after
/// the line it came on (ex_cmds.c `ex_global()`, `global_exe()`).
#[test]
fn a_global_with_a_range_inside_a_global_is_an_error() {
    let s = run("a\nb\na\n", ":g/a/2g/b/d<CR>");
    assert_eq!(s.text(), "a\nb\na\n");
    assert_eq!(
        s.errmsg().as_deref(),
        Some("E147: Cannot do :global recursive with a range")
    );
}

/// The commands `:g` runs on a line go on after an error, and the error
/// ends `:g` once that line is done (ex_docmd.c `do_cmdline()` without
/// `getexline`, ex_cmds.c `global_exe()`, `ex_copymove()`).
#[test]
fn an_error_in_global_ends_it_after_the_line() {
    let s = run("a\nb\nc", ":g/^/m99|d<CR>");
    assert_eq!(s.text(), "b\nc");
    assert!(s.errmsg().is_some());
}

/// `:j` inside `:g` joins each marked line that is left with the next one,
/// and `:j` on the last line does nothing, so the cursor stays there
/// (ex_cmds.c `global_exe_one()`, ex_docmd.c `ex_join()`).
#[test]
fn global_join_leaves_the_cursor_on_the_last_marked_line() {
    let s = run("a\n1\nb\n2\nc", ":g/^[a-z]/j<CR>");
    assert_eq!(s.text(), "a 1\nb 2\nc");
    assert_eq!(s.cursor_offset(), 8);
}

/// No line to run on is a message, not an error (ex_cmds.c `ex_global()`).
#[test]
fn global_without_lines_gives_a_message() {
    let message = |s: &TestSession| {
        s.last_effects().iter().find_map(|effect| match effect {
            Effect::ShowInfo {
                info: InfoMessage::Text(text),
            } => Some(text.to_string()),
            _ => None,
        })
    };
    let s = run("a\nb", ":g/z/d<CR>");
    assert_eq!(s.text(), "a\nb");
    assert_eq!(s.errmsg(), None);
    assert_eq!(message(&s).as_deref(), Some("Pattern not found: z"));
    let s = run("a\nb", ":v/./d<CR>");
    assert_eq!(s.text(), "a\nb");
    assert_eq!(s.errmsg(), None);
    assert_eq!(
        message(&s).as_deref(),
        Some("Pattern found in every line: .")
    );
}
