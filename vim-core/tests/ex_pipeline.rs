//! A `|` line runs each command on the text the command before it left, as
//! Vim's `do_cmdline()` does.

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
